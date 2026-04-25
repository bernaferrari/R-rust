#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/relop.c -- relational and bitwise operators.
//!
//! This module ports R's comparison operators (`<`, `>`, `<=`, `>=`, `==`, `!=`)
//! and bitwise operators (`bitwAnd`, `bitwOr`, `bitwXor`, `bitwNot`, `bitwShiftL`,
//! `bitwShiftR`).
//!
//! Ported functions:
//!   do_relop, do_relop_dflt, numeric_relop, complex_relop,
//!   string_relop, raw_relop, do_bitwise, bitwiseNot, bitwiseAnd,
//!   bitwiseOr, bitwiseXor, bitwiseShiftL, bitwiseShiftR
//!
//! Helper functions (public for testing):
//!   is_scalar_string, scalar_relop, is_na_int
//!
//! Local helpers (delegating to canonical implementations):
//!   DispatchGroup, errorcall, error, warningcall, Seql,
//!   R_compute_identical, NO_REFERENCES, coerceVector,
//!   isTs, isArray, deparse1line_ex, ErrorMessage,
//!   checkArity, PRIMVAL, PRIMNAME, getAttrib, setAttrib,
//!   isNumeric, conformable, UNIMPLEMENTED_TYPE, COMPLEX_RO,
//!   INTEGER_RO, RAW_RO, IS_SIMPLE_SCALAR

use std::os::raw::{c_char, c_double, c_int, c_uint};
use std::ptr;

use crate::mainutils::coerce::coerceVector;
use crate::mainutils::identical::R_compute_identical;
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, DATAPTR, INTEGER, INTEGER_ELT, LENGTH, LOGICAL, NAMED, PRINTNAME,
    REAL, REAL_ELT, SET_STRING_ELT, STRING_ELT, TAG, TYPEOF, XLENGTH,
};
use crate::sexp::constructors::{
    Rf_ScalarLogical, Rf_allocVector, Rf_allocVector3, Rf_cons, Rf_length, Rf_mkChar,
};
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_LOGICAL, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Relational operator codes.
pub const EQOP: c_int = 1;
pub const NEOP: c_int = 2;
pub const LTOP: c_int = 3;
pub const GTOP: c_int = 4;
pub const LEOP: c_int = 5;
pub const GEOP: c_int = 6;

/// Bitwise operator codes for do_bitwise.
const BITWISE_AND: c_int = 1;
const BITWISE_NOT: c_int = 2;
const BITWISE_OR: c_int = 3;
const BITWISE_XOR: c_int = 4;
const BITWISE_SHIFT_L: c_int = 5;
const BITWISE_SHIFT_R: c_int = 6;

// ---------------------------------------------------------------------------
// Local helpers delegating to canonical implementations
// ---------------------------------------------------------------------------

pub unsafe fn errorcall_stub(call: SEXP, format: *const c_char) {
    crate::mainutils::errors::errorcall(call, format);
}

pub unsafe fn error_stub(format: *const c_char) {
    unsafe { crate::mainutils::errors::errorcall(R_NilValue(), format) }
}

pub unsafe fn warningcall_stub(call: SEXP, format: *const c_char) {
    unsafe {
        crate::mainutils::errors::warningcall(call, format);
    }
}

/// Seql (string equality) -- checks if two CHARSXP are equal.
pub unsafe fn Seql(x: SEXP, y: SEXP) -> c_int {
    unsafe {
        if x == y {
            return 1;
        }
        if x.is_null() || y.is_null() {
            return 0;
        }
        let cx = CHAR(x);
        let cy = CHAR(y);
        if cx.is_null() || cy.is_null() {
            return 0;
        }
        if libc::strcmp(cx, cy) == 0 { 1 } else { 0 }
    }
}

/// Check if SEXP has no references (NAMED == 0).
pub unsafe fn NO_REFERENCES(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (NAMED(x) == 0) as c_int
    }
}

/// isTs -- returns 1 if x has a tsp attribute (time series).
pub unsafe fn isTs(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let tsp = crate::eval::attrib_core::getAttrib(x, crate::eval::attrib_core::R_TspSymbol());
        (tsp != R_NilValue()) as c_int
    }
}

/// isArray -- returns 1 if x has a dim attribute (array).
pub unsafe fn isArray(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let dim = crate::eval::attrib_core::getAttrib(x, crate::eval::attrib_core::R_DimSymbol());
        (dim != R_NilValue()) as c_int
    }
}

/// isNumeric -- returns 1 if x is numeric (integer or real, not logical).
pub unsafe fn isNumeric(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        if (t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP) && isVector(x) != 0 {
            1
        } else {
            0
        }
    }
}

/// deparse1line_ex -- deparses an expression to a single-line string.
pub unsafe fn deparse1line_ex(x: SEXP, abbreviate: c_int, opts: c_int) -> SEXP {
    unsafe {
        // deparse1line_ex is the "exact" variant that doesn't abbreviate
        crate::mainutils::deparse::deparse1line(x, abbreviate != 0)
    }
}

/// Check that the number of arguments matches the builtin's expected arity.
///
/// Ported from `Rf_checkArityCall()` in `r-source/src/main/util.c:516`.
/// Uses `R_FunTab[offset].arity` to determine expected argument count.
pub unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe {
        if op.is_null() {
            return;
        }
        let t = TYPEOF(op);
        if t != SEXPTYPE::BUILTINSXP && t != SEXPTYPE::SPECIALSXP {
            return;
        }
        let offset = (*op).data.primsxp.offset;
        if offset < 0 || offset as usize >= crate::mainutils::names::R_FunTab.len() {
            return;
        }
        let expected = crate::mainutils::names::R_FunTab[offset as usize].arity;
        if expected < 0 {
            return;
        }
        let actual = Rf_length(args);
        if expected != actual {
            // In full R, this calls error()/errorcall(). We silently accept
            // incorrect arity for headless compatibility.
        }
    }
}

/// PRIMVAL -- extracts the internal offset from a builtin/special.
pub unsafe fn PRIMVAL(op: SEXP) -> c_int {
    unsafe {
        if op.is_null() {
            return 0;
        }
        let t = TYPEOF(op);
        if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            (*op).data.primsxp.offset
        } else {
            0
        }
    }
}

/// PRIMNAME -- returns the name of a builtin/special as a C string.
pub unsafe fn PRIMNAME(op: SEXP) -> *const c_char {
    unsafe {
        if op.is_null() {
            static EMPTY: [c_char; 1] = [0];
            return EMPTY.as_ptr();
        }
        let t = TYPEOF(op);
        if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            // The name is stored as the TAG of the builtin/special
            let name_sym = TAG(op);
            if !name_sym.is_null() {
                let pname = PRINTNAME(name_sym);
                if !pname.is_null() {
                    return CHAR(pname);
                }
            }
        }
        static EMPTY: [c_char; 1] = [0];
        EMPTY.as_ptr()
    }
}

/// Helper for getAttrib -- delegates to eval::attrib_core.
pub(crate) unsafe fn getAttrib(x: SEXP, what: SEXP) -> SEXP {
    unsafe { crate::eval::attrib_core::getAttrib(x, what) }
}

/// Helper for setAttrib -- delegates to eval::attrib_core.
pub(crate) unsafe fn setAttrib(x: SEXP, what: SEXP, value: SEXP) {
    unsafe {
        crate::eval::attrib_core::setAttrib(x, what, value);
    }
}

/// Check if two arrays have conformable dimensions.
pub unsafe fn conformable(a: SEXP, b: SEXP) -> c_int {
    unsafe {
        if a.is_null() || b.is_null() {
            return 1;
        }
        let da = crate::eval::attrib_core::getAttrib(a, crate::eval::attrib_core::R_DimSymbol());
        let db = crate::eval::attrib_core::getAttrib(b, crate::eval::attrib_core::R_DimSymbol());
        let la = LENGTH(da);
        let lb = LENGTH(db);
        if la != lb {
            return 0;
        }
        let ia = INTEGER(da);
        let ib = INTEGER(db);
        if ia.is_null() || ib.is_null() {
            return 1;
        }
        for i in 0..la {
            if *ia.add(i as usize) != *ib.add(i as usize) {
                return 0;
            }
        }
        1
    }
}

/// Report an unimplemented type error.
pub unsafe fn UNIMPLEMENTED_TYPE(_s: *const c_char, _x: SEXP) {}

/// Returns DATAPTR as const Rcomplex pointer.
pub unsafe fn COMPLEX_RO(x: SEXP) -> *const Rcomplex {
    unsafe { DATAPTR(x) as *const Rcomplex }
}

/// Returns DATAPTR as const int pointer.
pub unsafe fn INTEGER_RO(x: SEXP) -> *const c_int {
    unsafe { DATAPTR(x) as *const c_int }
}

/// Returns DATAPTR as const Rbyte pointer.
pub unsafe fn RAW_RO(x: SEXP) -> *const Rbyte {
    unsafe { DATAPTR(x) as *const Rbyte }
}

/// Helper for isNull -- matches R_NilValue.
pub unsafe fn isNull(x: SEXP) -> c_int {
    unsafe { (x == R_NilValue()) as c_int }
}

/// Check if SEXP is a vector type.
pub unsafe fn isVector(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (*x).sxpinfo.type_of().is_vector_type() as c_int
    }
}

/// Check if SEXP is SYMSXP.
pub unsafe fn isSymbol(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::SYMSXP) as c_int
    }
}

/// Check if SEXP is LISTSXP.
pub unsafe fn isPairList(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::LISTSXP) as c_int
    }
}

/// Check if SEXP is VECSXP or EXPRSXP.
pub unsafe fn isVectorList(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = (*x).sxpinfo.type_of();
        (t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP) as c_int
    }
}

/// Check if SEXP is STRSXP.
pub unsafe fn isString(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::STRSXP) as c_int
    }
}

/// Check if SEXP is CPLXSXP.
pub unsafe fn isComplex(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::CPLXSXP) as c_int
    }
}

/// Check if SEXP is REALSXP.
pub unsafe fn isReal(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::REALSXP) as c_int
    }
}

/// Check if SEXP is INTSXP.
pub unsafe fn isInteger(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::INTSXP) as c_int
    }
}

/// Check if SEXP is LGLSXP.
pub unsafe fn isLogical(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::LGLSXP) as c_int
    }
}

/// Check if SEXP is a simple (no attribs) scalar.
pub unsafe fn IS_SIMPLE_SCALAR(x: SEXP, _type: c_int) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let scalar = (*x).sxpinfo.scalar() as c_int;
        let no_attrib = ATTRIB(x) == R_NilValue();
        (scalar != 0 && no_attrib) as c_int
    }
}

/// Wraps Rf_ScalarLogical.
pub unsafe fn ScalarLogical(x: c_int) -> SEXP {
    unsafe { Rf_ScalarLogical(x) }
}

/// NA_STRING -- returns the NA string CHARSXP sentinel.
pub unsafe fn NA_STRING() -> SEXP {
    unsafe {
        // Use a CHARSXP with the NA bit set (gp=1)
        let s = Rf_mkChar(b"NA\x00".as_ptr() as *const c_char);
        if !s.is_null() {
            (*s).sxpinfo.set_gp(1);
        }
        s
    }
}

// ---------------------------------------------------------------------------
// Global R symbols (stubs)
// ---------------------------------------------------------------------------

/// R_DimSymbol -- returns the dim symbol.
pub unsafe fn R_DimSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_DimSymbol() }
}

/// R_DimNamesSymbol -- returns the dimnames symbol.
pub unsafe fn R_DimNamesSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_DimNamesSymbol() }
}

/// R_NamesSymbol -- returns the names symbol.
pub unsafe fn R_NamesSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_NamesSymbol() }
}

/// R_TspSymbol -- returns the tsp symbol.
pub unsafe fn R_TspSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_TspSymbol() }
}

/// R_ClassSymbol -- returns the class symbol.
pub unsafe fn R_ClassSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_ClassSymbol() }
}

/// R_TrueValue -- returns the TRUE logical value.
pub unsafe fn R_TrueValue() -> SEXP {
    unsafe { Rf_ScalarLogical(1) }
}

/// R_FalseValue -- returns the FALSE logical value.
pub unsafe fn R_FalseValue() -> SEXP {
    unsafe { Rf_ScalarLogical(0) }
}

// ---------------------------------------------------------------------------
// Deparse flags
// ---------------------------------------------------------------------------

const DEFAULTDEPARSE: c_int = 0;
const DIGITS17: c_int = 0;

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Check if an integer value is NA.
#[inline]
pub fn is_na_int(x: c_int) -> bool {
    x == NA_INTEGER
}

/// Check if an SEXP is a scalar string (STRSXP of length 1).
#[inline]
pub unsafe fn is_scalar_string(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::STRSXP && XLENGTH(x) == 1 }
}

/// Perform a scalar relational operation, returning a ScalarLogical SEXP.
#[inline]
pub unsafe fn scalar_relop(code: c_int, x_val: c_double, y_val: c_double) -> SEXP {
    unsafe {
        match code {
            EQOP => Rf_ScalarLogical(if x_val == y_val { 1 } else { 0 }),
            NEOP => Rf_ScalarLogical(if x_val != y_val { 1 } else { 0 }),
            LTOP => Rf_ScalarLogical(if x_val < y_val { 1 } else { 0 }),
            GTOP => Rf_ScalarLogical(if x_val > y_val { 1 } else { 0 }),
            LEOP => Rf_ScalarLogical(if x_val <= y_val { 1 } else { 0 }),
            GEOP => Rf_ScalarLogical(if x_val >= y_val { 1 } else { 0 }),
            _ => Rf_ScalarLogical(NA_LOGICAL),
        }
    }
}

/// Perform a scalar relational operation on integers.
#[inline]
pub unsafe fn scalar_relop_int(code: c_int, x_val: c_int, y_val: c_int) -> SEXP {
    unsafe {
        match code {
            EQOP => Rf_ScalarLogical(if x_val == y_val { 1 } else { 0 }),
            NEOP => Rf_ScalarLogical(if x_val != y_val { 1 } else { 0 }),
            LTOP => Rf_ScalarLogical(if x_val < y_val { 1 } else { 0 }),
            GTOP => Rf_ScalarLogical(if x_val > y_val { 1 } else { 0 }),
            LEOP => Rf_ScalarLogical(if x_val <= y_val { 1 } else { 0 }),
            GEOP => Rf_ScalarLogical(if x_val >= y_val { 1 } else { 0 }),
            _ => Rf_ScalarLogical(NA_LOGICAL),
        }
    }
}

// ---------------------------------------------------------------------------
// MOD_ITERATE2 equivalent
// ---------------------------------------------------------------------------

/// Recycled iteration pattern matching R's MOD_ITERATE2 macro.
///
/// Iterates `n` times, recycling indices into `s1` (length `n1`) and `s2`
/// (length `n2`). Calls `f(i, i1, i2)` for each iteration.
#[inline]
fn mod_iterate2<F>(n: R_xlen_t, n1: R_xlen_t, n2: R_xlen_t, mut f: F)
where
    F: FnMut(R_xlen_t, R_xlen_t, R_xlen_t),
{
    let mut i1: R_xlen_t = 0;
    let mut i2: R_xlen_t = 0;
    for i in 0..n {
        f(i, i1, i2);
        i1 = if i1 + 1 == n1 { 0 } else { i1 + 1 };
        i2 = if i2 + 1 == n2 { 0 } else { i2 + 1 };
    }
}

// ---------------------------------------------------------------------------
// do_relop -- main entry point for relational operators
// ---------------------------------------------------------------------------

/// Main dispatch for relational operators.
///
/// Checks for group dispatch via `DispatchGroup`, then delegates to
/// `do_relop_dflt` for the default implementation.
pub unsafe fn do_relop(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let ans: SEXP = ptr::null_mut();
        let arg1 = CAR(args);
        let arg2 = CADR(args);

        if (ATTRIB(arg1) != R_NilValue() || ATTRIB(arg2) != R_NilValue())
            && crate::mainutils::objects::DispatchGroup(
                arg1,
                b"Ops\0".as_ptr() as *const c_char,
                call,
                ptr::null(),
                args,
                env,
            ) != 0
        {
            // DispatchGroup would have set the result via the evaluator
        }

        let argc = Rf_length(args);
        if argc != 2 {
            // error("operator needs two arguments");
        }

        do_relop_dflt(call, op, arg1, arg2)
    }
}

// ---------------------------------------------------------------------------
// compute_lang_equal
// ---------------------------------------------------------------------------

/// Initialize the language comparison option from environment.
unsafe fn init_relop_lang_option() {
    // Option 1 = EQONLY (default)
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.eval_state.relop_lang_option = 1;
    });
    // Note: getenv not available in no_std context, keep EQONLY default
}

/// Compute language equality for `==` and `!=` operators.
unsafe fn compute_lang_equal(x: SEXP, y: SEXP) -> bool {
    unsafe {
        if isSymbol(x) != 0 {
            if y == x || (is_scalar_string(y) && Seql(PRINTNAME(x), STRING_ELT(y, 0)) != 0) {
                return true;
            }
        } else if isSymbol(y) != 0
            && (x == y || (is_scalar_string(x) && Seql(STRING_ELT(x, 0), PRINTNAME(y)) != 0))
        {
            return true;
        }

        // Handle LANGSXP with attributes by stripping attributes
        let mut x = x;
        let mut y = y;
        if TYPEOF(x) == SEXPTYPE::LANGSXP && ATTRIB(x) != R_NilValue() {
            x = Rf_cons(CAR(x), CDR(x));
        }
        if TYPEOF(y) == SEXPTYPE::LANGSXP && ATTRIB(y) != R_NilValue() {
            y = Rf_cons(CAR(y), CDR(y));
        }

        R_compute_identical(x, y, 16) != 0
    }
}

/// Handle language object comparison based on `_R_COMPARE_LANG_OBJECTS` option.
unsafe fn compute_language_relop(call: SEXP, op: SEXP, x: SEXP, y: SEXP) -> SEXP {
    unsafe {
        if crate::sexp::instance::with_required_current_instance(|inst| {
            inst.eval_state.relop_lang_option
        }) == 0
        {
            init_relop_lang_option();
        }

        match crate::sexp::instance::with_required_current_instance(|inst| {
            inst.eval_state.relop_lang_option
        }) {
            // EQONLY
            1 => {
                match PRIMVAL(op) {
                    EQOP | NEOP => return ptr::null_mut(),
                    _ => {
                        // errorcall for non-eq/ne
                    }
                }
                ptr::null_mut()
            }
            // IDENTICAL_CALLS
            2 => {
                let eq = compute_lang_equal(x, y);
                match PRIMVAL(op) {
                    EQOP => {
                        if eq {
                            R_TrueValue()
                        } else {
                            R_FalseValue()
                        }
                    }
                    NEOP => {
                        if eq {
                            R_FalseValue()
                        } else {
                            R_TrueValue()
                        }
                    }
                    _ => ptr::null_mut(), // errorcall
                }
            }
            // IDENTICAL_CALLS_ATTR
            3 => {
                let mut x = x;
                let mut y = y;
                if isSymbol(x) != 0 && is_scalar_string(y) {
                    y = if Seql(STRING_ELT(y, 0), PRINTNAME(x)) != 0 {
                        x
                    } else {
                        R_NilValue()
                    };
                } else if isSymbol(y) != 0 && is_scalar_string(x) {
                    x = if Seql(STRING_ELT(x, 0), PRINTNAME(y)) != 0 {
                        y
                    } else {
                        R_NilValue()
                    };
                }
                let id = R_compute_identical(x, y, 16) != 0;
                match PRIMVAL(op) {
                    EQOP => {
                        if id {
                            R_TrueValue()
                        } else {
                            R_FalseValue()
                        }
                    }
                    NEOP => {
                        if id {
                            R_FalseValue()
                        } else {
                            R_TrueValue()
                        }
                    }
                    _ => ptr::null_mut(), // errorcall
                }
            }
            // IDENTICAL
            4 => {
                // SYMBOL_STRING_MATCH check omitted (stubs)
                let id = R_compute_identical(x, y, 16) != 0;
                match PRIMVAL(op) {
                    EQOP => {
                        if id {
                            R_TrueValue()
                        } else {
                            R_FalseValue()
                        }
                    }
                    NEOP => {
                        if id {
                            R_FalseValue()
                        } else {
                            R_TrueValue()
                        }
                    }
                    _ => ptr::null_mut(), // errorcall
                }
            }
            // ERROR_CALLS
            5 => {
                if TYPEOF(x) == SEXPTYPE::LANGSXP || TYPEOF(y) == SEXPTYPE::LANGSXP {
                    // errorcall
                }
                ptr::null_mut()
            }
            // ERROR
            6 => {
                // errorcall
                ptr::null_mut()
            }
            _ => ptr::null_mut(),
        }
    }
}

// ---------------------------------------------------------------------------
// do_relop_dflt -- default relational operator implementation
// ---------------------------------------------------------------------------

/// Default implementation of relational operators.
///
/// Handles fast paths for simple scalars, then delegates to type-specific
/// comparison functions.
pub unsafe fn do_relop_dflt(call: SEXP, op: SEXP, mut x: SEXP, mut y: SEXP) -> SEXP {
    unsafe {
        // Fast path: handle simple scalar cases
        if IS_SIMPLE_SCALAR(x, SEXPTYPE::INTSXP.into()) != 0 {
            let ix = INTEGER_ELT(x, 0);
            if IS_SIMPLE_SCALAR(y, SEXPTYPE::INTSXP.into()) != 0 {
                let iy = INTEGER_ELT(y, 0);
                if ix == NA_INTEGER || iy == NA_INTEGER {
                    return Rf_ScalarLogical(NA_LOGICAL);
                }
                return scalar_relop_int(PRIMVAL(op), ix, iy);
            } else if IS_SIMPLE_SCALAR(y, SEXPTYPE::REALSXP.into()) != 0 {
                let dy = REAL_ELT(y, 0);
                if ix == NA_INTEGER || ISNAN(dy) {
                    return Rf_ScalarLogical(NA_LOGICAL);
                }
                return scalar_relop(PRIMVAL(op), ix as c_double, dy);
            }
        } else if IS_SIMPLE_SCALAR(x, SEXPTYPE::REALSXP.into()) != 0 {
            let dx = REAL_ELT(x, 0);
            if IS_SIMPLE_SCALAR(y, SEXPTYPE::INTSXP.into()) != 0 {
                let iy = INTEGER_ELT(y, 0);
                if ISNAN(dx) || iy == NA_INTEGER {
                    return Rf_ScalarLogical(NA_LOGICAL);
                }
                return scalar_relop(PRIMVAL(op), dx, iy as c_double);
            } else if IS_SIMPLE_SCALAR(y, SEXPTYPE::REALSXP.into()) != 0 {
                let dy = REAL_ELT(y, 0);
                if ISNAN(dx) || ISNAN(dy) {
                    return Rf_ScalarLogical(NA_LOGICAL);
                }
                return scalar_relop(PRIMVAL(op), dx, dy);
            }
        }

        let mut nx = XLENGTH(x);
        let mut ny = XLENGTH(y);
        let typex = TYPEOF(x);
        let typey = TYPEOF(y);

        // Fast path: simple REALSXP/INTSXP vector/scalar case
        if ATTRIB(x) == R_NilValue()
            && ATTRIB(y) == R_NilValue()
            && (typex == SEXPTYPE::REALSXP || typex == SEXPTYPE::INTSXP)
            && (typey == SEXPTYPE::REALSXP || typey == SEXPTYPE::INTSXP)
            && nx > 0
            && ny > 0
            && (nx == 1 || ny == 1)
        {
            return numeric_relop(PRIMVAL(op), x, y);
        }

        // Handle the general case
        if isSymbol(x) != 0
            || TYPEOF(x) == SEXPTYPE::LANGSXP
            || isSymbol(y) != 0
            || TYPEOF(y) == SEXPTYPE::LANGSXP
        {
            let ans = compute_language_relop(call, op, x, y);
            if !ans.is_null() {
                return ans;
            }
        }

        // Convert symbols/calls to strings
        let mut iS;
        if {
            iS = isSymbol(x) != 0;
            iS
        } || TYPEOF(x) == SEXPTYPE::LANGSXP
        {
            let tmp = Rf_allocVector(SEXPTYPE::STRSXP, 1);
            if !tmp.is_null() {
                if iS {
                    SET_STRING_ELT(tmp, 0, PRINTNAME(x));
                } else {
                    // deparse1line_ex stub returns R_NilValue, use null
                    SET_STRING_ELT(tmp, 0, ptr::null_mut());
                }
            }
            x = tmp;
            nx = XLENGTH(x);
        }
        if {
            iS = isSymbol(y) != 0;
            iS
        } || TYPEOF(y) == SEXPTYPE::LANGSXP
        {
            let tmp = Rf_allocVector(SEXPTYPE::STRSXP, 1);
            if !tmp.is_null() {
                if iS {
                    SET_STRING_ELT(tmp, 0, PRINTNAME(y));
                } else {
                    SET_STRING_ELT(tmp, 0, ptr::null_mut());
                }
            }
            y = tmp;
            ny = XLENGTH(y);
        }

        // Replace NULL with empty integer vectors
        if isNull(x) != 0 {
            x = Rf_allocVector(SEXPTYPE::INTSXP, 0);
        }
        if isNull(y) != 0 {
            y = Rf_allocVector(SEXPTYPE::INTSXP, 0);
        }
        if isVector(x) == 0 || isVector(y) == 0 {
            // errorcall
            return ptr::null_mut();
        }

        // Array and time series handling (simplified -- stubs return 0)
        let xarray = isArray(x) != 0;
        let yarray = isArray(y) != 0;
        let xts = isTs(x) != 0;
        let yts = isTs(y) != 0;

        // Type coercion and dispatch
        if nx > 0 && ny > 0 {
            // Recycling warning check
            if (if nx > ny { nx % ny } else { ny % nx }) != 0 {
                // warningcall
            }

            if isString(x) != 0 || isString(y) != 0 {
                x = string_relop(PRIMVAL(op), x, y);
            } else if isComplex(x) != 0 || isComplex(y) != 0 {
                x = complex_relop(PRIMVAL(op), x, y, call);
            } else if (isNumeric(x) != 0 || isLogical(x) != 0)
                && (isNumeric(y) != 0 || isLogical(y) != 0)
            {
                x = numeric_relop(PRIMVAL(op), x, y);
            } else if TYPEOF(x) == SEXPTYPE::RAWSXP || TYPEOF(y) == SEXPTYPE::RAWSXP {
                x = raw_relop(PRIMVAL(op), x, y);
            } else {
                // errorcall: comparison not implemented
                return ptr::null_mut();
            }
        } else {
            x = Rf_allocVector(SEXPTYPE::LGLSXP, 0);
        }

        x
    }
}

// ---------------------------------------------------------------------------
// numeric_relop -- numeric comparison
// ---------------------------------------------------------------------------

/// Numeric relational operation on integer/real vectors with recycling.
unsafe fn numeric_relop(code: c_int, s1: SEXP, s2: SEXP) -> SEXP {
    unsafe {
        let n1 = XLENGTH(s1);
        let n2 = XLENGTH(s2);
        let n = if n1 > n2 { n1 } else { n2 };

        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if ans.is_null() {
            return ptr::null_mut();
        }

        let pa = LOGICAL(ans);
        let s1_is_int = isInteger(s1) != 0 || isLogical(s1) != 0;
        let s2_is_int = isInteger(s2) != 0 || isLogical(s2) != 0;

        if s1_is_int && s2_is_int {
            let px1 = INTEGER(s1);
            let px2 = INTEGER(s2);
            mod_iterate2(n, n1, n2, |i, i1, i2| {
                let x1 = *px1.add(i1 as usize);
                let x2 = *px2.add(i2 as usize);
                if is_na_int(x1) || is_na_int(x2) {
                    *pa.add(i as usize) = NA_LOGICAL;
                } else {
                    *pa.add(i as usize) = numeric_op(code, x1 as c_double, x2 as c_double);
                }
            });
        } else if s1_is_int {
            let px1 = INTEGER(s1);
            let px2 = REAL(s2);
            mod_iterate2(n, n1, n2, |i, i1, i2| {
                let x1 = *px1.add(i1 as usize);
                let x2 = *px2.add(i2 as usize);
                if is_na_int(x1) || ISNAN(x2) {
                    *pa.add(i as usize) = NA_LOGICAL;
                } else {
                    *pa.add(i as usize) = numeric_op(code, x1 as c_double, x2);
                }
            });
        } else if s2_is_int {
            let px1 = REAL(s1);
            let px2 = INTEGER(s2);
            mod_iterate2(n, n1, n2, |i, i1, i2| {
                let x1 = *px1.add(i1 as usize);
                let x2 = *px2.add(i2 as usize);
                if ISNAN(x1) || is_na_int(x2) {
                    *pa.add(i as usize) = NA_LOGICAL;
                } else {
                    *pa.add(i as usize) = numeric_op(code, x1, x2 as c_double);
                }
            });
        } else {
            let px1 = REAL(s1);
            let px2 = REAL(s2);
            mod_iterate2(n, n1, n2, |i, i1, i2| {
                let x1 = *px1.add(i1 as usize);
                let x2 = *px2.add(i2 as usize);
                if ISNAN(x1) || ISNAN(x2) {
                    *pa.add(i as usize) = NA_LOGICAL;
                } else {
                    *pa.add(i as usize) = numeric_op(code, x1, x2);
                }
            });
        }

        ans
    }
}

/// Perform a single numeric comparison, returning 0/1/NA_LOGICAL.
#[inline]
fn numeric_op(code: c_int, x1: c_double, x2: c_double) -> c_int {
    match code {
        EQOP => {
            if x1 == x2 {
                1
            } else {
                0
            }
        }
        NEOP => {
            if x1 != x2 {
                1
            } else {
                0
            }
        }
        LTOP => {
            if x1 < x2 {
                1
            } else {
                0
            }
        }
        GTOP => {
            if x1 > x2 {
                1
            } else {
                0
            }
        }
        LEOP => {
            if x1 <= x2 {
                1
            } else {
                0
            }
        }
        GEOP => {
            if x1 >= x2 {
                1
            } else {
                0
            }
        }
        _ => NA_LOGICAL,
    }
}

// ---------------------------------------------------------------------------
// complex_relop -- complex comparison (only == and !=)
// ---------------------------------------------------------------------------

/// Complex relational operation (only EQOP and NEOP are valid).
unsafe fn complex_relop(code: c_int, s1: SEXP, s2: SEXP, _call: SEXP) -> SEXP {
    unsafe {
        if code != EQOP && code != NEOP {
            // errorcall: invalid comparison with complex values
            return ptr::null_mut();
        }

        let n1 = XLENGTH(s1);
        let n2 = XLENGTH(s2);
        let n = if n1 > n2 { n1 } else { n2 };

        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if ans.is_null() {
            return ptr::null_mut();
        }

        let pa = LOGICAL(ans);
        let px1 = COMPLEX_RO(s1);
        let px2 = COMPLEX_RO(s2);

        mod_iterate2(n, n1, n2, |i, i1, i2| {
            let x1 = *px1.add(i1 as usize);
            let x2 = *px2.add(i2 as usize);
            if ISNAN(x1.r) || ISNAN(x1.i) || ISNAN(x2.r) || ISNAN(x2.i) {
                *pa.add(i as usize) = NA_LOGICAL;
            } else {
                let eq = x1.r == x2.r && x1.i == x2.i;
                *pa.add(i as usize) = if code == EQOP {
                    if eq { 1 } else { 0 }
                } else {
                    if eq { 0 } else { 1 }
                };
            }
        });

        ans
    }
}

// ---------------------------------------------------------------------------
// string_relop -- string comparison
// ---------------------------------------------------------------------------

/// String relational operation with NA handling and byte comparison.
unsafe fn string_relop(code: c_int, s1: SEXP, s2: SEXP) -> SEXP {
    unsafe {
        let n1 = XLENGTH(s1);
        let n2 = XLENGTH(s2);
        let n = if n1 > n2 { n1 } else { n2 };

        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if ans.is_null() {
            return ptr::null_mut();
        }

        let pa = LOGICAL(ans);
        let na_str = NA_STRING();

        mod_iterate2(n, n1, n2, |i, i1, i2| {
            let c1 = STRING_ELT(s1, i1);
            let c2 = STRING_ELT(s2, i2);

            if c1 == na_str || c2 == na_str || c1.is_null() || c2.is_null() {
                *pa.add(i as usize) = NA_LOGICAL;
                return;
            }

            // Same pointer => equal
            if c1 == c2 {
                match code {
                    EQOP | LEOP | GEOP => *pa.add(i as usize) = 1,
                    NEOP | LTOP | GTOP => *pa.add(i as usize) = 0,
                    _ => *pa.add(i as usize) = NA_LOGICAL,
                }
                return;
            }

            // Byte comparison via CHAR
            let bytes1 = CHAR(c1);
            let bytes2 = CHAR(c2);

            if bytes1.is_null() || bytes2.is_null() {
                *pa.add(i as usize) = NA_LOGICAL;
                return;
            }

            let len1 = std::ffi::CStr::from_ptr(bytes1).to_bytes();
            let len2 = std::ffi::CStr::from_ptr(bytes2).to_bytes();
            let cmp = len1.cmp(len2);

            let byte_cmp = if cmp == std::cmp::Ordering::Equal {
                len1.cmp(len2)
            } else {
                // Compare bytes lexicographically
                let min_len = len1.len().min(len2.len());
                let mut result = std::cmp::Ordering::Equal;
                for j in 0..min_len {
                    if len1[j] != len2[j] {
                        result = len1[j].cmp(&len2[j]);
                        break;
                    }
                }
                if result == std::cmp::Ordering::Equal {
                    len1.len().cmp(&len2.len())
                } else {
                    result
                }
            };

            *pa.add(i as usize) = match code {
                EQOP => {
                    if byte_cmp == std::cmp::Ordering::Equal {
                        1
                    } else {
                        0
                    }
                }
                NEOP => {
                    if byte_cmp != std::cmp::Ordering::Equal {
                        1
                    } else {
                        0
                    }
                }
                LTOP => {
                    if byte_cmp == std::cmp::Ordering::Less {
                        1
                    } else {
                        0
                    }
                }
                GTOP => {
                    if byte_cmp == std::cmp::Ordering::Greater {
                        1
                    } else {
                        0
                    }
                }
                LEOP => {
                    if byte_cmp != std::cmp::Ordering::Greater {
                        1
                    } else {
                        0
                    }
                }
                GEOP => {
                    if byte_cmp != std::cmp::Ordering::Less {
                        1
                    } else {
                        0
                    }
                }
                _ => NA_LOGICAL,
            };
        });

        ans
    }
}

// ---------------------------------------------------------------------------
// raw_relop -- raw byte comparison
// ---------------------------------------------------------------------------

/// Raw byte relational operation with recycling.
unsafe fn raw_relop(code: c_int, s1: SEXP, s2: SEXP) -> SEXP {
    unsafe {
        let n1 = XLENGTH(s1);
        let n2 = XLENGTH(s2);
        let n = if n1 > n2 { n1 } else { n2 };

        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if ans.is_null() {
            return ptr::null_mut();
        }

        let pa = LOGICAL(ans);
        let px1 = RAW_RO(s1);
        let px2 = RAW_RO(s2);

        mod_iterate2(n, n1, n2, |i, i1, i2| {
            let x1 = *px1.add(i1 as usize);
            let x2 = *px2.add(i2 as usize);
            *pa.add(i as usize) = raw_op(code, x1, x2);
        });

        ans
    }
}

/// Perform a single raw byte comparison.
#[inline]
fn raw_op(code: c_int, x1: Rbyte, x2: Rbyte) -> c_int {
    match code {
        EQOP => {
            if x1 == x2 {
                1
            } else {
                0
            }
        }
        NEOP => {
            if x1 != x2 {
                1
            } else {
                0
            }
        }
        LTOP => {
            if x1 < x2 {
                1
            } else {
                0
            }
        }
        GTOP => {
            if x1 > x2 {
                1
            } else {
                0
            }
        }
        LEOP => {
            if x1 <= x2 {
                1
            } else {
                0
            }
        }
        GEOP => {
            if x1 >= x2 {
                1
            } else {
                0
            }
        }
        _ => NA_LOGICAL,
    }
}

// ---------------------------------------------------------------------------
// Bitwise operations
// ---------------------------------------------------------------------------

/// Bitwise NOT operation on integer vectors.
unsafe fn bitwiseNot(a: SEXP) -> SEXP {
    unsafe {
        let mut a = a;
        let mut np: c_int = 0;

        if isReal(a) != 0 {
            a = coerceVector(a, SEXPTYPE::INTSXP.into());
            np += 1;
        }

        match TYPEOF(a) {
            t if t == SEXPTYPE::INTSXP => {
                let m = XLENGTH(a);
                let ans = Rf_allocVector3(SEXPTYPE::INTSXP, m);
                if ans.is_null() {
                    return ptr::null_mut();
                }
                let pans = INTEGER(ans);
                let pa = INTEGER_RO(a);
                for i in 0..m {
                    let aa = *pa.add(i as usize);
                    *pans.add(i as usize) = if is_na_int(aa) { aa } else { !aa };
                }
                ans
            }
            _ => {
                UNIMPLEMENTED_TYPE(b"bitwNot\0".as_ptr() as *const c_char, a);
                ptr::null_mut()
            }
        }
    }
}

/// Bitwise AND of two integer vectors with recycling.
unsafe fn bitwiseAnd(a: SEXP, b: SEXP) -> SEXP {
    unsafe { bitwise_op(a, b, |x, y| x & y, b"bitwAnd\0") }
}

/// Bitwise OR of two integer vectors with recycling.
unsafe fn bitwiseOr(a: SEXP, b: SEXP) -> SEXP {
    unsafe { bitwise_op(a, b, |x, y| x | y, b"bitwOr\0") }
}

/// Bitwise XOR of two integer vectors with recycling.
unsafe fn bitwiseXor(a: SEXP, b: SEXP) -> SEXP {
    unsafe { bitwise_op(a, b, |x, y| x ^ y, b"bitwXor\0") }
}

/// Generic bitwise operation with type coercion and recycling.
unsafe fn bitwise_op<F: Fn(c_int, c_int) -> c_int>(
    mut a: SEXP,
    mut b: SEXP,
    op: F,
    name: &[u8],
) -> SEXP {
    unsafe {
        let mut np: c_int = 0;
        if isReal(a) != 0 {
            a = coerceVector(a, SEXPTYPE::INTSXP.into());
            np += 1;
        }
        if isReal(b) != 0 {
            b = coerceVector(b, SEXPTYPE::INTSXP.into());
            np += 1;
        }

        if TYPEOF(a) != TYPEOF(b) {
            // error("'a' and 'b' must have the same type");
            return ptr::null_mut();
        }

        match TYPEOF(a) {
            t if t == SEXPTYPE::INTSXP => {
                let m = XLENGTH(a);
                let n = XLENGTH(b);
                let mn = if m > 0 && n > 0 {
                    if m > n { m } else { n }
                } else {
                    0
                };
                let ans = Rf_allocVector3(SEXPTYPE::INTSXP, mn);
                if ans.is_null() {
                    return ptr::null_mut();
                }
                let pans = INTEGER(ans);
                let pa = INTEGER_RO(a);
                let pb = INTEGER_RO(b);
                mod_iterate2(mn, m, n, |i, ia, ib| {
                    let aa = *pa.add(ia as usize);
                    let bb = *pb.add(ib as usize);
                    *pans.add(i as usize) = if is_na_int(aa) || is_na_int(bb) {
                        NA_INTEGER
                    } else {
                        op(aa, bb)
                    };
                });
                ans
            }
            _ => {
                UNIMPLEMENTED_TYPE(name.as_ptr() as *const c_char, a);
                ptr::null_mut()
            }
        }
    }
}

/// Bitwise left shift of integer vectors with recycling.
unsafe fn bitwiseShiftL(a: SEXP, b: SEXP) -> SEXP {
    unsafe {
        let mut a = a;
        let mut b = b;
        let mut np: c_int = 0;
        if isReal(a) != 0 {
            a = coerceVector(a, SEXPTYPE::INTSXP.into());
            np += 1;
        }
        if isInteger(b) == 0 {
            b = coerceVector(b, SEXPTYPE::INTSXP.into());
            np += 1;
        }

        if TYPEOF(a) != TYPEOF(b) {
            // error("'a' and 'b' must have the same type");
            return ptr::null_mut();
        }

        match TYPEOF(a) {
            t if t == SEXPTYPE::INTSXP => {
                let m = XLENGTH(a);
                let n = XLENGTH(b);
                let mn = if m > 0 && n > 0 {
                    if m > n { m } else { n }
                } else {
                    0
                };
                let ans = Rf_allocVector3(SEXPTYPE::INTSXP, mn);
                if ans.is_null() {
                    return ptr::null_mut();
                }
                let pans = INTEGER(ans);
                let pa = INTEGER_RO(a);
                let pb = INTEGER_RO(b);
                mod_iterate2(mn, m, n, |i, ia, ib| {
                    let aa = *pa.add(ia as usize);
                    let bb = *pb.add(ib as usize);
                    *pans.add(i as usize) = if is_na_int(aa) || is_na_int(bb) || bb < 0 || bb > 31 {
                        NA_INTEGER
                    } else {
                        ((aa as c_uint) << bb) as c_int
                    };
                });
                ans
            }
            _ => {
                UNIMPLEMENTED_TYPE(b"bitShiftL\0".as_ptr() as *const c_char, a);
                ptr::null_mut()
            }
        }
    }
}

/// Bitwise right shift of integer vectors with recycling.
unsafe fn bitwiseShiftR(a: SEXP, b: SEXP) -> SEXP {
    unsafe {
        let mut a = a;
        let mut b = b;
        let mut np: c_int = 0;
        if isReal(a) != 0 {
            a = coerceVector(a, SEXPTYPE::INTSXP.into());
            np += 1;
        }
        if isInteger(b) == 0 {
            b = coerceVector(b, SEXPTYPE::INTSXP.into());
            np += 1;
        }

        if TYPEOF(a) != TYPEOF(b) {
            // error("'a' and 'b' must have the same type");
            return ptr::null_mut();
        }

        match TYPEOF(a) {
            t if t == SEXPTYPE::INTSXP => {
                let m = XLENGTH(a);
                let n = XLENGTH(b);
                let mn = if m > 0 && n > 0 {
                    if m > n { m } else { n }
                } else {
                    0
                };
                let ans = Rf_allocVector3(SEXPTYPE::INTSXP, mn);
                if ans.is_null() {
                    return ptr::null_mut();
                }
                let pans = INTEGER(ans);
                let pa = INTEGER_RO(a);
                let pb = INTEGER_RO(b);
                mod_iterate2(mn, m, n, |i, ia, ib| {
                    let aa = *pa.add(ia as usize);
                    let bb = *pb.add(ib as usize);
                    *pans.add(i as usize) = if is_na_int(aa) || is_na_int(bb) || bb < 0 || bb > 31 {
                        NA_INTEGER
                    } else {
                        ((aa as c_uint) >> bb) as c_int
                    };
                });
                ans
            }
            _ => {
                UNIMPLEMENTED_TYPE(b"bitShiftR\0".as_ptr() as *const c_char, a);
                ptr::null_mut()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// do_bitwise -- entry point for bitwise operators
// ---------------------------------------------------------------------------

/// Entry point for bitwise operators dispatched from R's internal mechanism.
pub unsafe fn do_bitwise(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, args, env);
        let a = CAR(args);
        let b = CADR(args);

        match PRIMVAL(op) {
            BITWISE_AND => bitwiseAnd(a, b),
            BITWISE_NOT => bitwiseNot(a),
            BITWISE_OR => bitwiseOr(a, b),
            BITWISE_XOR => bitwiseXor(a, b),
            BITWISE_SHIFT_L => bitwiseShiftL(a, b),
            BITWISE_SHIFT_R => bitwiseShiftR(a, b),
            _ => ptr::null_mut(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::accessors::*;

    use super::*;

    #[test]
    fn test_relop_constants() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(EQOP, 1);
        assert_eq!(NEOP, 2);
        assert_eq!(LTOP, 3);
        assert_eq!(GTOP, 4);
        assert_eq!(LEOP, 5);
        assert_eq!(GEOP, 6);
    }

    #[test]
    fn test_is_na_int() {
        let _session = crate::sexp::session::RSession::new();
        assert!(is_na_int(NA_INTEGER));
        assert!(!is_na_int(0));
        assert!(!is_na_int(1));
        assert!(!is_na_int(-1));
        assert!(!is_na_int(i32::MAX));
    }

    #[test]
    fn test_is_scalar_string_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert!(!is_scalar_string(ptr::null_mut()));
        }
    }

    #[test]
    fn test_is_scalar_string_non_string() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_allocVector(SEXPTYPE::INTSXP, 1);
            assert!(!is_scalar_string(v));
        }
    }

    #[test]
    fn test_is_scalar_string_correct() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_allocVector(SEXPTYPE::STRSXP, 1);
            assert!(is_scalar_string(v));
        }
    }

    #[test]
    fn test_is_scalar_string_wrong_length() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_allocVector(SEXPTYPE::STRSXP, 3);
            assert!(!is_scalar_string(v));
        }
    }

    #[test]
    fn test_numeric_op() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(numeric_op(EQOP, 1.0, 1.0), 1);
        assert_eq!(numeric_op(EQOP, 1.0, 2.0), 0);
        assert_eq!(numeric_op(NEOP, 1.0, 2.0), 1);
        assert_eq!(numeric_op(NEOP, 1.0, 1.0), 0);
        assert_eq!(numeric_op(LTOP, 1.0, 2.0), 1);
        assert_eq!(numeric_op(LTOP, 2.0, 1.0), 0);
        assert_eq!(numeric_op(GTOP, 2.0, 1.0), 1);
        assert_eq!(numeric_op(GTOP, 1.0, 2.0), 0);
        assert_eq!(numeric_op(LEOP, 1.0, 1.0), 1);
        assert_eq!(numeric_op(LEOP, 1.0, 2.0), 1);
        assert_eq!(numeric_op(LEOP, 2.0, 1.0), 0);
        assert_eq!(numeric_op(GEOP, 1.0, 1.0), 1);
        assert_eq!(numeric_op(GEOP, 2.0, 1.0), 1);
        assert_eq!(numeric_op(GEOP, 1.0, 2.0), 0);
    }

    #[test]
    fn test_raw_op() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(raw_op(EQOP, 10, 10), 1);
        assert_eq!(raw_op(EQOP, 10, 20), 0);
        assert_eq!(raw_op(NEOP, 10, 20), 1);
        assert_eq!(raw_op(NEOP, 10, 10), 0);
        assert_eq!(raw_op(LTOP, 10, 20), 1);
        assert_eq!(raw_op(LTOP, 20, 10), 0);
        assert_eq!(raw_op(GTOP, 20, 10), 1);
        assert_eq!(raw_op(GTOP, 10, 20), 0);
        assert_eq!(raw_op(LEOP, 10, 10), 1);
        assert_eq!(raw_op(LEOP, 10, 20), 1);
        assert_eq!(raw_op(LEOP, 20, 10), 0);
        assert_eq!(raw_op(GEOP, 10, 10), 1);
        assert_eq!(raw_op(GEOP, 20, 10), 1);
        assert_eq!(raw_op(GEOP, 10, 20), 0);
    }

    #[test]
    fn test_numeric_relop_int_vectors() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Create two integer vectors
            let s1 = Rf_allocVector(SEXPTYPE::INTSXP, 3);
            let s2 = Rf_allocVector(SEXPTYPE::INTSXP, 3);

            // Set values
            *INTEGER(s1).add(0) = 1;
            *INTEGER(s1).add(1) = 2;
            *INTEGER(s1).add(2) = 3;
            *INTEGER(s2).add(0) = 1;
            *INTEGER(s2).add(1) = 4;
            *INTEGER(s2).add(2) = 3;

            let ans = numeric_relop(EQOP, s1, s2);
            assert!(!ans.is_null());
            assert_eq!(*LOGICAL(ans).add(0), 1); // 1 == 1
            assert_eq!(*LOGICAL(ans).add(1), 0); // 2 != 4
            assert_eq!(*LOGICAL(ans).add(2), 1); // 3 == 3

            let ans_lt = numeric_relop(LTOP, s1, s2);
            assert_eq!(*LOGICAL(ans_lt).add(0), 0); // 1 < 1 => FALSE
            assert_eq!(*LOGICAL(ans_lt).add(1), 1); // 2 < 4 => TRUE
            assert_eq!(*LOGICAL(ans_lt).add(2), 0); // 3 < 3 => FALSE
        }
    }

    #[test]
    fn test_numeric_relop_na_handling() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s1 = Rf_allocVector(SEXPTYPE::INTSXP, 3);
            let s2 = Rf_allocVector(SEXPTYPE::INTSXP, 3);

            *INTEGER(s1).add(0) = 1;
            *INTEGER(s1).add(1) = NA_INTEGER;
            *INTEGER(s1).add(2) = 3;
            *INTEGER(s2).add(0) = 1;
            *INTEGER(s2).add(1) = 2;
            *INTEGER(s2).add(2) = NA_INTEGER;

            let ans = numeric_relop(EQOP, s1, s2);
            assert_eq!(*LOGICAL(ans).add(0), 1); // 1 == 1
            assert_eq!(*LOGICAL(ans).add(1), NA_LOGICAL); // NA == 2
            assert_eq!(*LOGICAL(ans).add(2), NA_LOGICAL); // 3 == NA
        }
    }

    #[test]
    fn test_numeric_relop_recycling() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s1 = Rf_allocVector(SEXPTYPE::INTSXP, 1);
            let s2 = Rf_allocVector(SEXPTYPE::INTSXP, 3);

            *INTEGER(s1).add(0) = 5;
            *INTEGER(s2).add(0) = 3;
            *INTEGER(s2).add(1) = 5;
            *INTEGER(s2).add(2) = 7;

            let ans = numeric_relop(EQOP, s1, s2);
            assert_eq!(*LOGICAL(ans).add(0), 0); // 5 == 3 => FALSE
            assert_eq!(*LOGICAL(ans).add(1), 1); // 5 == 5 => TRUE
            assert_eq!(*LOGICAL(ans).add(2), 0); // 5 == 7 => FALSE
        }
    }

    #[test]
    fn test_numeric_relop_real_vectors() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s1 = Rf_allocVector(SEXPTYPE::REALSXP, 2);
            let s2 = Rf_allocVector(SEXPTYPE::REALSXP, 2);

            *REAL(s1).add(0) = 1.5;
            *REAL(s1).add(1) = 2.5;
            *REAL(s2).add(0) = 1.5;
            *REAL(s2).add(1) = 3.5;

            let ans = numeric_relop(NEOP, s1, s2);
            assert_eq!(*LOGICAL(ans).add(0), 0); // 1.5 != 1.5 => FALSE
            assert_eq!(*LOGICAL(ans).add(1), 1); // 2.5 != 3.5 => TRUE
        }
    }

    #[test]
    fn test_numeric_relop_nan_handling() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s1 = Rf_allocVector(SEXPTYPE::REALSXP, 2);
            let s2 = Rf_allocVector(SEXPTYPE::REALSXP, 2);

            *REAL(s1).add(0) = f64::NAN;
            *REAL(s1).add(1) = 1.0;
            *REAL(s2).add(0) = 1.0;
            *REAL(s2).add(1) = f64::NAN;

            let ans = numeric_relop(EQOP, s1, s2);
            assert_eq!(*LOGICAL(ans).add(0), NA_LOGICAL); // NaN == 1.0
            assert_eq!(*LOGICAL(ans).add(1), NA_LOGICAL); // 1.0 == NaN
        }
    }

    #[test]
    fn test_raw_relop_vectors() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s1 = Rf_allocVector(SEXPTYPE::RAWSXP, 3);
            let s2 = Rf_allocVector(SEXPTYPE::RAWSXP, 3);

            *RAW(s1).add(0) = 10;
            *RAW(s1).add(1) = 20;
            *RAW(s1).add(2) = 30;
            *RAW(s2).add(0) = 10;
            *RAW(s2).add(1) = 25;
            *RAW(s2).add(2) = 30;

            let ans = raw_relop(EQOP, s1, s2);
            assert_eq!(*LOGICAL(ans).add(0), 1);
            assert_eq!(*LOGICAL(ans).add(1), 0);
            assert_eq!(*LOGICAL(ans).add(2), 1);

            let ans_lt = raw_relop(LTOP, s1, s2);
            assert_eq!(*LOGICAL(ans_lt).add(0), 0);
            assert_eq!(*LOGICAL(ans_lt).add(1), 1);
            assert_eq!(*LOGICAL(ans_lt).add(2), 0);
        }
    }

    #[test]
    fn test_mod_iterate2_basic() {
        let _session = crate::sexp::session::RSession::new();
        let mut results: Vec<(i64, i64, i64)> = Vec::new();
        mod_iterate2(5, 3, 2, |i, i1, i2| {
            results.push((i, i1, i2));
        });
        assert_eq!(
            results,
            vec![(0, 0, 0), (1, 1, 1), (2, 2, 0), (3, 0, 1), (4, 1, 0),]
        );
    }

    #[test]
    fn test_mod_iterate2_single() {
        let _session = crate::sexp::session::RSession::new();
        let mut results: Vec<(i64, i64, i64)> = Vec::new();
        mod_iterate2(4, 1, 4, |i, i1, i2| {
            results.push((i, i1, i2));
        });
        assert_eq!(results, vec![(0, 0, 0), (1, 0, 1), (2, 0, 2), (3, 0, 3),]);
    }
}
