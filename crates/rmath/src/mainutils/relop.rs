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
//!   isNumeric, conformable, UNIMPLEMENTED_TYPE, IS_SIMPLE_SCALAR

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int, c_uint};
use std::ptr;

use crate::mainutils::coerce::coerceVector;
use crate::mainutils::identical::R_compute_identical;
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, INTEGER_ELT, LENGTH, NAMED, PRINTNAME, REAL_ELT, SET_STRING_ELT, STRING_ELT,
    TYPEOF, XLENGTH,
};
use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector, Rf_length};
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_LOGICAL, R_xlen_t, Rbyte, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::object::{SessionNodeFactory, Sexp, SexpMut, SexpResult};
use crate::sexp::owner::{OwnerToken, StoredOwner};
use crate::sexp::protect::protect;

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

pub fn errorcall_stub(call: SEXP, format: *const c_char) {
    crate::mainutils::errors::errorcall(call, format);
}

pub fn error_stub(format: *const c_char) {
    crate::mainutils::errors::errorcall(R_NilValue(), format)
}

pub fn warningcall_stub(call: SEXP, format: *const c_char) {
    // SAFETY: delegates to the raw warningcall boundary; `call` and `format`
    // carry the same contract the unsafe callee already requires.

    crate::mainutils::errors::warningcall(call, format);
}

/// GNU `Seql`: pointer equality, else UTF-8 translation (memory.c).
pub fn Seql(x: SEXP, y: SEXP) -> c_int {
    if x == y {
        return 1;
    }
    if x.is_null() || y.is_null() {
        return 0;
    }
    if crate::sexp::accessors::IS_BYTES(x) != 0 || crate::sexp::accessors::IS_BYTES(y) != 0 {
        if crate::sexp::accessors::IS_BYTES(x) != 0 && crate::sexp::accessors::IS_BYTES(y) != 0 {
            return (crate::mainutils::printutils::charsxp_bytes(x)
                == crate::mainutils::printutils::charsxp_bytes(y)) as c_int;
        }
        return 0;
    }
    (crate::sexp::accessors::charsxp_as_utf8(x) == crate::sexp::accessors::charsxp_as_utf8(y))
        as c_int
}

/// Check if SEXP has no references (NAMED == 0).
pub fn NO_REFERENCES(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    (NAMED(x) == 0) as c_int
}

/// isTs -- returns 1 if x has a tsp attribute (time series).
pub fn isTs(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    let tsp = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_TspSymbol());
    (tsp != R_NilValue()) as c_int
}

/// isArray -- returns 1 if x has a dim attribute (array).
pub fn isArray(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
    (dim != R_NilValue()) as c_int
}

/// isNumeric -- returns 1 if x is numeric (integer or real, not logical).
pub fn isNumeric(x: SEXP) -> c_int {
    {
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
pub fn deparse1line_ex(x: SEXP, abbreviate: c_int, opts: c_int) -> SEXP {
    // SAFETY: delegates to the raw deparse boundary with the same arguments.

    // deparse1line_ex is the "exact" variant that doesn't abbreviate
    crate::mainutils::deparse::deparse1line(x, abbreviate != 0)
}

/// Check that the number of arguments matches the builtin's expected arity.
///
/// Ported from `Rf_checkArityCall()` in `r-source/src/main/util.c:516`.
/// Uses `R_FunTab[offset].arity` to determine expected argument count.
pub fn checkArity(op: SEXP, args: SEXP) {
    if op.is_null() {
        return;
    }
    let t = TYPEOF(op);
    if t != SEXPTYPE::BUILTINSXP && t != SEXPTYPE::SPECIALSXP {
        return;
    }
    let Some(entry) = crate::eval::primitive::raw_primitive_entry(op) else {
        return;
    };
    let expected = entry.arity;
    if expected < 0 {
        return;
    }
    let actual = Rf_length(args);
    if expected != actual {
        let internal = (entry.eval % 100) / 10 != 0;
        let name = crate::eval::primitive::fun_tab_name(entry.name);
        let noun = if actual == 1 { "argument" } else { "arguments" };
        let message = if internal {
            format!("{actual} {noun} passed to .Internal({name}) which requires {expected}")
        } else {
            format!("{actual} {noun} passed to '{name}' which requires {expected}")
        };
        std::panic::panic_any(crate::sexp::context::RError { message });
    }
}

/// PRIMVAL -- extracts the internal offset from a builtin/special.
pub fn PRIMVAL(op: SEXP) -> c_int {
    let kind = TYPEOF(op);
    if kind == SEXPTYPE::BUILTINSXP || kind == SEXPTYPE::SPECIALSXP {
        let table_index = crate::sexp::accessors::PRIMOFFSET(op);
        crate::eval::primitive::fun_tab_descriptor(table_index)
            .map_or(table_index, |entry| entry.offset)
    } else {
        0
    }
}

/// PRIMNAME -- returns the name of a builtin/special as a C string.
pub fn PRIMNAME(op: SEXP) -> *const c_char {
    static EMPTY: [c_char; 1] = [0];
    crate::eval::primitive::raw_primitive_entry(op)
        .map_or(EMPTY.as_ptr(), |entry| entry.name.as_ptr().cast())
}

/// Helper for getAttrib -- delegates to eval::attrib_core.
pub(crate) fn getAttrib(x: SEXP, what: SEXP) -> SEXP {
    crate::sexp::attrib_core::getAttrib(x, what)
}

/// Helper for setAttrib -- delegates to eval::attrib_core.
pub(crate) fn setAttrib(x: SEXP, what: SEXP, value: SEXP) {
    // SAFETY: delegates to the raw attrib boundary with the same arguments.
    unsafe {
        crate::eval::attrib_core::setAttrib(x, what, value);
    }
}

/// Check if two arrays have conformable dimensions.
pub fn conformable(a: SEXP, b: SEXP) -> c_int {
    if a.is_null() || b.is_null() {
        return 1;
    }
    let da = crate::sexp::attrib_core::getAttrib(a, crate::sexp::attrib_core::R_DimSymbol());
    let db = crate::sexp::attrib_core::getAttrib(b, crate::sexp::attrib_core::R_DimSymbol());
    let la = LENGTH(da);
    let lb = LENGTH(db);
    if la != lb {
        return 0;
    }
    // A dim without integer storage has no payload to compare.
    if TYPEOF(da) != SEXPTYPE::INTSXP || TYPEOF(db) != SEXPTYPE::INTSXP {
        return 1;
    }
    for i in 0..la {
        if INTEGER_ELT(da, i) != INTEGER_ELT(db, i) {
            return 0;
        }
    }
    1
}

/// Report an unimplemented type error.
pub fn UNIMPLEMENTED_TYPE(routine: *const c_char, x: SEXP) -> ! {
    let routine = if routine.is_null() {
        "relop"
    } else {
        // SAFETY: callers pass NUL-terminated routine name literals.
        unsafe { CStr::from_ptr(routine) }
            .to_str()
            .unwrap_or("relop")
    };
    let sexptype = if x.is_null() { -1 } else { TYPEOF(x) };
    std::panic::panic_any(crate::sexp::context::RError {
        message: format!("{routine}: unsupported SEXPTYPE {sexptype}"),
    });
}

fn bitwise_type_mismatch() -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: "'a' and 'b' must have the same type".to_string(),
    });
}

fn relop_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
}

// ---------------------------------------------------------------------------
// Session-scoped checked helpers
// ---------------------------------------------------------------------------

/// Acquire the current session's node factory for one builtin scope.
///
/// # Safety boundary
/// [`OwnerToken::current`] names whichever instance is current on this
/// thread. These routines only run on the interpreter thread during builtin
/// evaluation, where the current instance is the rightful allocation owner.
fn session_factory() -> SexpResult<SessionNodeFactory<'static>> {
    // SAFETY: builtin evaluation runs on the interpreter thread with a live
    // current instance; the token only scopes this factory's allocations.
    let owner = unsafe { OwnerToken::current() }?;
    Ok(owner.node_factory())
}

/// Wrap a raw SEXP owned by the current session's heap.
///
/// `SessionNodeFactory::wrap` validates heap membership, so the returned
/// handle carries enough authority for checked reads, allocation, and
/// `SexpMut` mutation.
fn checked(x: SEXP) -> Sexp<'static> {
    session_factory()
        .and_then(|factory| factory.wrap(x))
        .unwrap_or_else(|err| relop_error(err.to_string()))
}

/// Allocation authority carried by an already checked handle.
fn vector_factory<'a>(x: &Sexp<'a>) -> SexpResult<SessionNodeFactory<'a>> {
    StoredOwner::from_value(x)?.node_factory()
}

/// Allocate a fresh mutable vector in `x`'s session.
fn alloc_in<'a>(x: &Sexp<'a>, kind: SEXPTYPE, len: R_xlen_t) -> SexpResult<SexpMut<'a>> {
    let value = vector_factory(x)?.allocate(|arena| Some(arena.alloc_vector(kind, len)))?;
    SexpMut::try_from_checked(value)
}

/// Allocate a one-element logical vector on the current session.
fn scalar_logical_current(v: c_int) -> SexpResult<Sexp<'static>> {
    let value =
        session_factory()?.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::LGLSXP, 1)))?;
    let mut scalar = SexpMut::try_from_checked(value)?;
    scalar.try_set_logical_elt(0, v)?;
    Ok(scalar.freeze())
}

/// Hand a checked result back across the raw boundary.
fn unwrap_sexp(result: SexpResult<Sexp<'_>>) -> SEXP {
    result
        .map(|value| value.as_raw())
        .unwrap_or_else(|err| relop_error(err.to_string()))
}

/// Read an element of an integer or logical vector (shared `c_int` layout).
fn int_like_elt(x: &Sexp<'_>, i: R_xlen_t) -> SexpResult<c_int> {
    match x.typeof_() {
        SEXPTYPE::LGLSXP => x.try_logical_elt(i),
        _ => x.try_integer_elt(i),
    }
}

/// Thin safe wrapper over the raw `Seql` CHARSXP comparator.
fn seql_chars(x: &Sexp<'_>, y: &Sexp<'_>) -> c_int {
    Seql(x.as_raw(), y.as_raw())
}

/// Thin safe wrapper over the raw `charsxp_as_utf8` translation.
fn charsxp_utf8(x: &Sexp<'_>) -> Vec<u8> {
    // SAFETY: the handle names a live CHARSXP retained by its root lease.
    crate::sexp::accessors::charsxp_as_utf8(x.as_raw())
}

/// Coerce a checked vector to INTSXP, preserving the handle's session.
fn coerce_to_int<'a>(x: &Sexp<'a>) -> SexpResult<Sexp<'a>> {
    // SAFETY: `coerceVector` returns a fresh session-heap vector here, which
    // the factory wrap re-admits as a checked handle.
    let raw = coerceVector(x.as_raw(), SEXPTYPE::INTSXP.into());
    vector_factory(x)?.wrap(raw)
}

/// Coerce `x` to INTSXP when it is REALSXP (bitwise `a` operand rule).
fn coerce_bitwise_operand<'a>(x: &Sexp<'a>) -> SexpResult<Sexp<'a>> {
    if x.typeof_() == SEXPTYPE::REALSXP {
        coerce_to_int(x)
    } else {
        Ok(x.clone())
    }
}

fn primitive_name(op: SEXP) -> String {
    crate::eval::primitive::raw_primitive_entry(op).map_or_else(String::new, |entry| {
        let bytes = entry.name.split(|&b| b == 0).next().unwrap_or_default();
        String::from_utf8_lossy(bytes).into_owned()
    })
}

fn language_comparison_error(op: SEXP) -> ! {
    relop_error(format!(
        "comparison ({}) is not possible for language types",
        primitive_name(op)
    ))
}

/// Helper for isNull -- matches R_NilValue.
pub fn isNull(x: SEXP) -> c_int {
    c_int::from(x == R_NilValue())
}

/// Check if SEXP is a vector type.
pub fn isVector(x: SEXP) -> c_int {
    c_int::from(SEXPTYPE(TYPEOF(x)).is_vector_type())
}

/// Check if SEXP is SYMSXP.
pub fn isSymbol(x: SEXP) -> c_int {
    crate::sexp::constructors::Rf_isSymbol(x)
}

/// Check if SEXP is LISTSXP.
pub fn isPairList(x: SEXP) -> c_int {
    c_int::from(TYPEOF(x) == SEXPTYPE::LISTSXP)
}

/// Check if SEXP is VECSXP or EXPRSXP.
pub fn isVectorList(x: SEXP) -> c_int {
    let kind = TYPEOF(x);
    c_int::from(kind == SEXPTYPE::VECSXP || kind == SEXPTYPE::EXPRSXP)
}

/// Check if SEXP is STRSXP.
pub fn isString(x: SEXP) -> c_int {
    crate::sexp::constructors::Rf_isString(x)
}

/// Check if SEXP is CPLXSXP.
pub fn isComplex(x: SEXP) -> c_int {
    crate::sexp::constructors::Rf_isComplex(x)
}

/// Check if SEXP is REALSXP.
pub fn isReal(x: SEXP) -> c_int {
    crate::sexp::constructors::Rf_isReal(x)
}

/// Check if SEXP is INTSXP.
pub fn isInteger(x: SEXP) -> c_int {
    crate::sexp::constructors::Rf_isInteger(x)
}

/// Check if SEXP is LGLSXP.
pub fn isLogical(x: SEXP) -> c_int {
    crate::sexp::constructors::Rf_isLogical(x)
}

/// Check if SEXP is a simple (no attribs) scalar.
pub fn IS_SIMPLE_SCALAR(x: SEXP, _type: c_int) -> c_int {
    if x.is_null() {
        return 0;
    }
    let scalar = checked(x).header().sxpinfo.scalar() as c_int;
    let no_attrib = ATTRIB(x) == R_NilValue();
    (scalar != 0 && no_attrib) as c_int
}

/// Wraps Rf_ScalarLogical.
pub fn ScalarLogical(x: c_int) -> SEXP {
    unwrap_sexp(scalar_logical_current(x))
}

/// NA_STRING -- returns the NA string CHARSXP sentinel.
pub fn NA_STRING() -> SEXP {
    crate::sexp::globals::R_NaString()
}

// ---------------------------------------------------------------------------
// Global R symbols (stubs)
// ---------------------------------------------------------------------------

/// R_DimSymbol -- returns the dim symbol.
pub fn R_DimSymbol() -> SEXP {
    crate::sexp::attrib_core::R_DimSymbol()
}

/// R_DimNamesSymbol -- returns the dimnames symbol.
pub fn R_DimNamesSymbol() -> SEXP {
    crate::sexp::attrib_core::R_DimNamesSymbol()
}

/// R_NamesSymbol -- returns the names symbol.
pub fn R_NamesSymbol() -> SEXP {
    crate::sexp::attrib_core::R_NamesSymbol()
}

/// R_TspSymbol -- returns the tsp symbol.
pub fn R_TspSymbol() -> SEXP {
    crate::sexp::attrib_core::R_TspSymbol()
}

/// R_ClassSymbol -- returns the class symbol.
pub fn R_ClassSymbol() -> SEXP {
    crate::sexp::attrib_core::R_ClassSymbol()
}

/// R_TrueValue -- returns the TRUE logical value.
pub fn R_TrueValue() -> SEXP {
    unwrap_sexp(scalar_logical_current(1))
}

/// R_FalseValue -- returns the FALSE logical value.
pub fn R_FalseValue() -> SEXP {
    unwrap_sexp(scalar_logical_current(0))
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
pub fn is_scalar_string(x: SEXP) -> bool {
    !x.is_null() && TYPEOF(x) == SEXPTYPE::STRSXP && XLENGTH(x) == 1
}

/// Perform a scalar relational operation, returning a ScalarLogical SEXP.
#[inline]
pub fn scalar_relop(code: c_int, x_val: c_double, y_val: c_double) -> SEXP {
    let value = match code {
        EQOP => (x_val == y_val) as c_int,
        NEOP => (x_val != y_val) as c_int,
        LTOP => (x_val < y_val) as c_int,
        GTOP => (x_val > y_val) as c_int,
        LEOP => (x_val <= y_val) as c_int,
        GEOP => (x_val >= y_val) as c_int,
        _ => NA_LOGICAL,
    };
    unwrap_sexp(scalar_logical_current(value))
}

/// Perform a scalar relational operation on integers.
#[inline]
pub fn scalar_relop_int(code: c_int, x_val: c_int, y_val: c_int) -> SEXP {
    scalar_relop(code, x_val as c_double, y_val as c_double)
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

/// Fallible variant of [`mod_iterate2`] for checked element access.
#[inline]
fn mod_iterate2_try<F>(n: R_xlen_t, n1: R_xlen_t, n2: R_xlen_t, mut f: F) -> SexpResult<()>
where
    F: FnMut(R_xlen_t, R_xlen_t, R_xlen_t) -> SexpResult<()>,
{
    let mut i1: R_xlen_t = 0;
    let mut i2: R_xlen_t = 0;
    for i in 0..n {
        f(i, i1, i2)?;
        i1 = if i1 + 1 == n1 { 0 } else { i1 + 1 };
        i2 = if i2 + 1 == n2 { 0 } else { i2 + 1 };
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// do_relop -- main entry point for relational operators
// ---------------------------------------------------------------------------

/// Main dispatch for relational operators.
///
/// Checks for group dispatch via `DispatchGroup`, then delegates to
/// `do_relop_dflt` for the default implementation.
pub fn do_relop(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let arg1 = CAR(args);
    let arg2 = CADR(args);

    // Prefer the evaluator DispatchGroup that actually applies methods.
    // Skip ordered factors so the dedicated Ops.ordered path keeps stock warnings.
    if !checked_has_class(arg1, "ordered") && !checked_has_class(arg2, "ordered") {
        let mut ans: SEXP = R_NilValue();
        // SAFETY: "Ops" is a NUL-terminated literal, `ans` is a live local
        // out-slot, and call/op/args/env are the evaluator's rooted inputs.
        let dispatched = unsafe {
            crate::eval::dispatch::DispatchGroup(
                b"Ops\0".as_ptr() as *const c_char,
                call,
                op,
                args,
                env,
                &mut ans,
            )
        };
        if dispatched != 0 {
            return ans;
        }
    }

    let argc = Rf_length(args);
    if argc != 2 {
        relop_error("operator needs two arguments");
    }

    do_relop_dflt(call, op, arg1, arg2)
}

// ---------------------------------------------------------------------------
// compute_lang_equal
// ---------------------------------------------------------------------------

/// Initialize the language comparison option from environment.
fn init_relop_lang_option() {
    // GNU default since PR#18676: == and != use identical(), ignoring srcref.
    set_relop_lang_option(2);
}

/// Store the instance-scoped language comparison mode.
fn set_relop_lang_option(value: c_int) {
    crate::sexp::instance::with_required_current_instance(|inst| {
        // SAFETY: the current instance pointer is live for the closure; this
        // is a single scalar field write with no allocation or reentry.
        unsafe { (*inst).eval_state.relop_lang_option = value };
    });
}

/// Read the instance-scoped language comparison mode.
fn relop_lang_option() -> c_int {
    crate::sexp::instance::with_required_current_instance(|inst| {
        // SAFETY: the current instance pointer is live for the closure; this
        // is a single scalar field read with no allocation or reentry.
        unsafe { (*inst).eval_state.relop_lang_option }
    })
}

/// `identical(x, y)` with the language-comparison flags (ignore srcref).
fn identical_lang(x: SEXP, y: SEXP) -> bool {
    // SAFETY: `x` and `y` are live operands rooted by the relop caller for the
    // duration of the comparison.
    unsafe { R_compute_identical(x, y, 16) != 0 }
}

/// Compute language equality for `==` and `!=` operators.
fn compute_lang_equal(x: SEXP, y: SEXP) -> bool {
    if isSymbol(x) != 0 {
        if y == x || (is_scalar_string(y) && Seql(PRINTNAME(x), STRING_ELT(y, 0)) != 0) {
            return true;
        }
    } else if isSymbol(y) != 0
        && (x == y || (is_scalar_string(x) && Seql(STRING_ELT(x, 0), PRINTNAME(y)) != 0))
    {
        return true;
    }

    identical_lang(x, y)
}

/// Handle language object comparison based on `_R_COMPARE_LANG_OBJECTS` option.
fn compute_language_relop(call: SEXP, op: SEXP, x: SEXP, y: SEXP) -> SEXP {
    if relop_lang_option() == 0 {
        init_relop_lang_option();
    }

    match relop_lang_option() {
        // EQONLY
        1 => match PRIMVAL(op) {
            EQOP | NEOP => return ptr::null_mut(),
            _ => language_comparison_error(op),
        },
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
                _ => language_comparison_error(op),
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
            let id = identical_lang(x, y);
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
                _ => language_comparison_error(op),
            }
        }
        // IDENTICAL
        4 => {
            // SYMBOL_STRING_MATCH check omitted (stubs)
            let id = identical_lang(x, y);
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
                _ => language_comparison_error(op),
            }
        }
        // ERROR_CALLS
        5 => {
            if TYPEOF(x) == SEXPTYPE::LANGSXP || TYPEOF(y) == SEXPTYPE::LANGSXP {
                relop_error("comparison of call objects is not supported");
            }
            ptr::null_mut()
        }
        // ERROR
        6 => {
            relop_error("comparison of language objects is not supported");
        }
        _ => ptr::null_mut(),
    }
}

// ---------------------------------------------------------------------------
// do_relop_dflt -- default relational operator implementation
// ---------------------------------------------------------------------------

/// Default implementation of relational operators.
///
/// Handles fast paths for simple scalars, then delegates to type-specific
/// comparison functions.
pub fn do_relop_dflt(call: SEXP, op: SEXP, mut x: SEXP, mut y: SEXP) -> SEXP {
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

    let mut nx = if isVector(x) != 0 { XLENGTH(x) } else { 0 };
    let mut ny = if isVector(y) != 0 { XLENGTH(y) } else { 0 };
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
        return unwrap_sexp(numeric_relop(PRIMVAL(op), &checked(x), &checked(y)));
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

    let mut held = Vec::new();
    // Convert symbols/calls to strings
    let mut iS;
    if {
        iS = isSymbol(x) != 0;
        iS
    } || TYPEOF(x) == SEXPTYPE::LANGSXP
    {
        let tmp = Rf_allocVector(SEXPTYPE::STRSXP, 1);
        held.push(crate::sexp::protect::protect(tmp));
        if !tmp.is_null() {
            if iS {
                SET_STRING_ELT(tmp, 0, PRINTNAME(x));
            } else {
                let dep = crate::mainutils::deparse::deparse1line(x, false);
                let _dep = crate::sexp::protect::protect(dep);
                SET_STRING_ELT(tmp, 0, STRING_ELT(dep, 0));
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
        held.push(crate::sexp::protect::protect(tmp));
        if !tmp.is_null() {
            if iS {
                SET_STRING_ELT(tmp, 0, PRINTNAME(y));
            } else {
                let dep = crate::mainutils::deparse::deparse1line(y, false);
                let _dep = crate::sexp::protect::protect(dep);
                SET_STRING_ELT(tmp, 0, STRING_ELT(dep, 0));
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
        relop_error(format!(
            "comparison ({}) is possible only for atomic and list types",
            primitive_name(op)
        ));
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

        if let Some(result) = ordered_factor_relop(PRIMVAL(op), &checked(x), &checked(y))
            .unwrap_or_else(|err| -> Option<Sexp<'_>> { relop_error(err.to_string()) })
        {
            x = result.as_raw();
        } else if is_unordered_factor_eq_ne(PRIMVAL(op), &checked(x), &checked(y))
            .unwrap_or_else(|err| -> bool { relop_error(err.to_string()) })
        {
            // Upstream comparison.c: unordered factor ==/!= compares
            // the LEVEL STRINGS of the elements (factor vs character
            // matches the character against the levels); relational
            // operators on unordered factors are "not meaningful".
            let xf = unwrap_sexp(factor_operand_as_levels(&checked(x)));
            let _xg = protect(xf);
            let yf = unwrap_sexp(factor_operand_as_levels(&checked(y)));
            let _yg = protect(yf);
            x = unwrap_sexp(string_relop(PRIMVAL(op), &checked(xf), &checked(yf)));
        } else if isString(x) != 0 || isString(y) != 0 {
            x = unwrap_sexp(string_relop(PRIMVAL(op), &checked(x), &checked(y)));
        } else if isComplex(x) != 0 || isComplex(y) != 0 {
            x = unwrap_sexp(complex_relop(PRIMVAL(op), &checked(x), &checked(y)));
        } else if (isNumeric(x) != 0 || isLogical(x) != 0)
            && (isNumeric(y) != 0 || isLogical(y) != 0)
        {
            x = unwrap_sexp(numeric_relop(PRIMVAL(op), &checked(x), &checked(y)));
        } else if isReal(x) != 0 || isReal(y) != 0 {
            let xc = coerceVector(x, SEXPTYPE::REALSXP.into());
            let _xg = protect(xc);
            let yc = coerceVector(y, SEXPTYPE::REALSXP.into());
            let _yg = protect(yc);
            x = unwrap_sexp(numeric_relop(PRIMVAL(op), &checked(xc), &checked(yc)));
        } else if isInteger(x) != 0 || isInteger(y) != 0 {
            let xc = coerceVector(x, SEXPTYPE::INTSXP.into());
            let _xg = protect(xc);
            let yc = coerceVector(y, SEXPTYPE::INTSXP.into());
            let _yg = protect(yc);
            x = unwrap_sexp(numeric_relop(PRIMVAL(op), &checked(xc), &checked(yc)));
        } else if isLogical(x) != 0 || isLogical(y) != 0 {
            let xc = coerceVector(x, SEXPTYPE::LGLSXP.into());
            let _xg = protect(xc);
            let yc = coerceVector(y, SEXPTYPE::LGLSXP.into());
            let _yg = protect(yc);
            x = unwrap_sexp(numeric_relop(PRIMVAL(op), &checked(xc), &checked(yc)));
        } else if TYPEOF(x) == SEXPTYPE::RAWSXP || TYPEOF(y) == SEXPTYPE::RAWSXP {
            x = unwrap_sexp(raw_relop(PRIMVAL(op), &checked(x), &checked(y)));
        } else {
            relop_error("comparison of these types is not implemented");
        }
    } else {
        x = Rf_allocVector(SEXPTYPE::LGLSXP, 0);
    }

    x
}

/// Raw-boundary `has_class` for callers still holding SEXP pointers.
fn checked_has_class(value: SEXP, class_name: &str) -> bool {
    if value.is_null() {
        return false;
    }
    has_class(&checked(value), class_name).unwrap_or(false)
}

/// Whether this is an unordered-factor (either side) equality/inequality
/// comparison — the case that compares level strings.
fn is_unordered_factor_eq_ne(code: c_int, x: &Sexp<'_>, y: &Sexp<'_>) -> SexpResult<bool> {
    Ok((code == EQOP || code == NEOP)
        && (has_class(x, "factor")? || has_class(y, "factor")?)
        && !has_class(x, "ordered")?
        && !has_class(y, "ordered")?)
}

/// Convert a factor operand to its level strings (`as.character(f)`); any
/// non-factor operand maps to itself (a character RHS compares directly
/// against the levels, matching upstream match()-against-levels).
fn factor_operand_as_levels<'a>(v: &Sexp<'a>) -> SexpResult<Sexp<'a>> {
    if !has_class(v, "factor")? {
        return Ok(v.clone());
    }
    let Some(levels) = factor_levels(v)? else {
        return Ok(v.clone());
    };
    let n = v.len();
    let mut out = alloc_in(v, SEXPTYPE::STRSXP, n)?;
    for i in 0..n {
        let code = v.try_integer_elt(i)?;
        let value = if code == NA_INTEGER || code < 1 || code as usize > levels.len() {
            checked(crate::sexp::globals::R_NaString())
        } else {
            vector_factory(v)?.character(&levels[(code - 1) as usize])?
        };
        out.try_set_string_elt(i, value)?;
    }
    Ok(out.freeze())
}

fn ordered_factor_relop<'a>(
    code: c_int,
    x: &Sexp<'a>,
    y: &Sexp<'a>,
) -> SexpResult<Option<Sexp<'a>>> {
    let x_ordered = has_class(x, "ordered")?;
    let y_ordered = has_class(y, "ordered")?;
    if !x_ordered && !y_ordered {
        return Ok(None);
    }

    let levels_owner = if x_ordered { x } else { y };
    let Some(levels) = factor_levels(levels_owner)? else {
        return Ok(None);
    };
    if (x_ordered && !factor_levels_match(x, &levels)?)
        || (y_ordered && !factor_levels_match(y, &levels)?)
    {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "level sets of factors are different".to_string(),
        });
    }

    let n1 = x.len();
    let n2 = y.len();
    let n = n1.max(n2);
    let mut ans = alloc_in(x, SEXPTYPE::LGLSXP, n)?;

    mod_iterate2_try(n, n1, n2, |i, i1, i2| {
        let lhs = ordered_operand_code(x, i1, &levels)?;
        let rhs = ordered_operand_code(y, i2, &levels)?;
        ans.try_set_logical_elt(i, ordered_code_relop(code, lhs, rhs))
    })?;
    Ok(Some(ans.freeze()))
}

#[derive(Clone, Copy)]
enum OrderedOperand {
    Code(c_int),
    Missing,
    UnknownLevel,
}

fn ordered_code_relop(code: c_int, lhs: OrderedOperand, rhs: OrderedOperand) -> c_int {
    match (lhs, rhs) {
        (OrderedOperand::Missing, _) | (_, OrderedOperand::Missing) => NA_LOGICAL,
        (OrderedOperand::UnknownLevel, _) | (_, OrderedOperand::UnknownLevel) => match code {
            EQOP => 0,
            NEOP => 1,
            _ => NA_LOGICAL,
        },
        (OrderedOperand::Code(lhs), OrderedOperand::Code(rhs)) => {
            numeric_op(code, lhs as c_double, rhs as c_double)
        }
    }
}

fn ordered_operand_code(
    value: &Sexp<'_>,
    index: R_xlen_t,
    levels: &[String],
) -> SexpResult<OrderedOperand> {
    if has_class(value, "ordered")? {
        let code = value.try_integer_elt(index)?;
        Ok(
            if code == NA_INTEGER || code <= 0 || code as usize > levels.len() {
                OrderedOperand::Missing
            } else {
                OrderedOperand::Code(code)
            },
        )
    } else if value.typeof_() == SEXPTYPE::STRSXP {
        let charsxp = value.try_string_elt(index)?;
        let Some(label) = charsxp_to_string(&charsxp) else {
            return Ok(OrderedOperand::Missing);
        };
        Ok(levels
            .iter()
            .position(|level| level == &label)
            .map(|position| OrderedOperand::Code(position as c_int + 1))
            .unwrap_or(OrderedOperand::UnknownLevel))
    } else {
        Ok(OrderedOperand::UnknownLevel)
    }
}

fn factor_levels_match(value: &Sexp<'_>, expected: &[String]) -> SexpResult<bool> {
    Ok(factor_levels(value)?.is_some_and(|levels| levels == expected))
}

fn factor_levels(value: &Sexp<'_>) -> SexpResult<Option<Vec<String>>> {
    let levels = getAttrib(value.as_raw(), crate::sexp::symbol::install_str("levels"));
    if levels.is_null() || levels == R_NilValue() {
        return Ok(None);
    }
    let levels = checked(levels);
    if levels.typeof_() != SEXPTYPE::STRSXP {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(levels.len() as usize);
    for i in 0..levels.len() {
        let Some(label) = charsxp_to_string(&levels.try_string_elt(i)?) else {
            return Ok(None);
        };
        out.push(label);
    }
    Ok(Some(out))
}

fn has_class(value: &Sexp<'_>, class_name: &str) -> SexpResult<bool> {
    let class = getAttrib(value.as_raw(), R_ClassSymbol());
    if class.is_null() || class == R_NilValue() {
        return Ok(false);
    }
    let class = checked(class);
    if class.typeof_() != SEXPTYPE::STRSXP {
        return Ok(false);
    }
    for i in 0..class.len() {
        if charsxp_to_string(&class.try_string_elt(i)?).as_deref() == Some(class_name) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn charsxp_to_string(charsxp: &Sexp<'_>) -> Option<String> {
    if charsxp.as_raw().is_null() || charsxp.is_na_string() {
        return None;
    }
    let bytes = charsxp.header().char_bytes()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

// ---------------------------------------------------------------------------
// numeric_relop -- numeric comparison
// ---------------------------------------------------------------------------

/// Numeric relational operation on integer/real vectors with recycling.
fn numeric_relop<'a>(code: c_int, s1: &Sexp<'a>, s2: &Sexp<'a>) -> SexpResult<Sexp<'a>> {
    let n1 = s1.len();
    let n2 = s2.len();
    let n = if n1 > n2 { n1 } else { n2 };

    let mut ans = alloc_in(s1, SEXPTYPE::LGLSXP, n)?;

    let s1_is_int = matches!(s1.typeof_(), SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP);
    let s2_is_int = matches!(s2.typeof_(), SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP);

    if s1_is_int && s2_is_int {
        mod_iterate2_try(n, n1, n2, |i, i1, i2| {
            let x1 = int_like_elt(s1, i1)?;
            let x2 = int_like_elt(s2, i2)?;
            let v = if is_na_int(x1) || is_na_int(x2) {
                NA_LOGICAL
            } else {
                numeric_op(code, x1 as c_double, x2 as c_double)
            };
            ans.try_set_logical_elt(i, v)
        })?;
    } else if s1_is_int {
        mod_iterate2_try(n, n1, n2, |i, i1, i2| {
            let x1 = int_like_elt(s1, i1)?;
            let x2 = s2.try_real_elt(i2)?;
            let v = if is_na_int(x1) || ISNAN(x2) {
                NA_LOGICAL
            } else {
                numeric_op(code, x1 as c_double, x2)
            };
            ans.try_set_logical_elt(i, v)
        })?;
    } else if s2_is_int {
        mod_iterate2_try(n, n1, n2, |i, i1, i2| {
            let x1 = s1.try_real_elt(i1)?;
            let x2 = int_like_elt(s2, i2)?;
            let v = if ISNAN(x1) || is_na_int(x2) {
                NA_LOGICAL
            } else {
                numeric_op(code, x1, x2 as c_double)
            };
            ans.try_set_logical_elt(i, v)
        })?;
    } else {
        mod_iterate2_try(n, n1, n2, |i, i1, i2| {
            let x1 = s1.try_real_elt(i1)?;
            let x2 = s2.try_real_elt(i2)?;
            let v = if ISNAN(x1) || ISNAN(x2) {
                NA_LOGICAL
            } else {
                numeric_op(code, x1, x2)
            };
            ans.try_set_logical_elt(i, v)
        })?;
    }

    Ok(ans.freeze())
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
fn complex_relop<'a>(code: c_int, s1: &Sexp<'a>, s2: &Sexp<'a>) -> SexpResult<Sexp<'a>> {
    if code != EQOP && code != NEOP {
        relop_error("invalid comparison with complex values");
    }

    let n1 = s1.len();
    let n2 = s2.len();
    let n = if n1 > n2 { n1 } else { n2 };

    let mut ans = alloc_in(s1, SEXPTYPE::LGLSXP, n)?;

    mod_iterate2_try(n, n1, n2, |i, i1, i2| {
        let x1 = s1.try_complex_elt(i1)?;
        let x2 = s2.try_complex_elt(i2)?;
        let v = if ISNAN(x1.r) || ISNAN(x1.i) || ISNAN(x2.r) || ISNAN(x2.i) {
            NA_LOGICAL
        } else {
            let eq = x1.r == x2.r && x1.i == x2.i;
            if code == EQOP {
                if eq { 1 } else { 0 }
            } else {
                if eq { 0 } else { 1 }
            }
        };
        ans.try_set_logical_elt(i, v)
    })?;

    Ok(ans.freeze())
}

// ---------------------------------------------------------------------------
// string_relop -- string comparison
// ---------------------------------------------------------------------------

/// String relational operation with NA handling and byte comparison.
fn string_relop<'a>(code: c_int, s1: &Sexp<'a>, s2: &Sexp<'a>) -> SexpResult<Sexp<'a>> {
    let n1 = s1.len();
    let n2 = s2.len();
    let n = if n1 > n2 { n1 } else { n2 };

    let mut ans = alloc_in(s1, SEXPTYPE::LGLSXP, n)?;

    mod_iterate2_try(n, n1, n2, |i, i1, i2| {
        let c1 = s1.try_string_elt(i1)?;
        let c2 = s2.try_string_elt(i2)?;

        if c1.as_raw().is_null() || c2.as_raw().is_null() || c1.is_na_string() || c2.is_na_string()
        {
            return ans.try_set_logical_elt(i, NA_LOGICAL);
        }

        if c1.as_raw() == c2.as_raw() {
            let v = match code {
                EQOP | LEOP | GEOP => 1,
                NEOP | LTOP | GTOP => 0,
                _ => NA_LOGICAL,
            };
            return ans.try_set_logical_elt(i, v);
        }

        if code == EQOP {
            let v = seql_chars(&c1, &c2);
            return ans.try_set_logical_elt(i, v);
        }
        if code == NEOP {
            let v = if seql_chars(&c1, &c2) != 0 { 0 } else { 1 };
            return ans.try_set_logical_elt(i, v);
        }

        let u1 = charsxp_utf8(&c1);
        let u2 = charsxp_utf8(&c2);
        let byte_cmp = u1.cmp(&u2);
        let v = match code {
            LTOP => (byte_cmp == std::cmp::Ordering::Less) as c_int,
            GTOP => (byte_cmp == std::cmp::Ordering::Greater) as c_int,
            LEOP => (byte_cmp != std::cmp::Ordering::Greater) as c_int,
            GEOP => (byte_cmp != std::cmp::Ordering::Less) as c_int,
            _ => NA_LOGICAL,
        };
        ans.try_set_logical_elt(i, v)
    })?;

    Ok(ans.freeze())
}

// ---------------------------------------------------------------------------
// raw_relop -- raw byte comparison
// ---------------------------------------------------------------------------

/// Raw byte relational operation with recycling.
fn raw_relop<'a>(code: c_int, s1: &Sexp<'a>, s2: &Sexp<'a>) -> SexpResult<Sexp<'a>> {
    let n1 = s1.len();
    let n2 = s2.len();
    let n = if n1 > n2 { n1 } else { n2 };

    let mut ans = alloc_in(s1, SEXPTYPE::LGLSXP, n)?;

    mod_iterate2_try(n, n1, n2, |i, i1, i2| {
        let x1 = s1.try_raw_elt(i1)?;
        let x2 = s2.try_raw_elt(i2)?;
        ans.try_set_logical_elt(i, raw_op(code, x1, x2))
    })?;

    Ok(ans.freeze())
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
fn bitwise_not<'a>(a: &Sexp<'a>) -> SexpResult<Sexp<'a>> {
    let a = coerce_bitwise_operand(a)?;
    match a.typeof_() {
        SEXPTYPE::INTSXP => {
            let m = a.len();
            let mut ans = alloc_in(&a, SEXPTYPE::INTSXP, m)?;
            for i in 0..m {
                let aa = a.try_integer_elt(i)?;
                ans.try_set_integer_elt(i, if is_na_int(aa) { aa } else { !aa })?;
            }
            Ok(ans.freeze())
        }
        _ => UNIMPLEMENTED_TYPE(b"bitwNot\0".as_ptr() as *const c_char, a.as_raw()),
    }
}

/// Bitwise AND of two integer vectors with recycling.
fn bitwise_and<'a>(a: &Sexp<'a>, b: &Sexp<'a>) -> SexpResult<Sexp<'a>> {
    bitwise_op(a, b, |x, y| x & y, b"bitwAnd\0")
}

/// Bitwise OR of two integer vectors with recycling.
fn bitwise_or<'a>(a: &Sexp<'a>, b: &Sexp<'a>) -> SexpResult<Sexp<'a>> {
    bitwise_op(a, b, |x, y| x | y, b"bitwOr\0")
}

/// Bitwise XOR of two integer vectors with recycling.
fn bitwise_xor<'a>(a: &Sexp<'a>, b: &Sexp<'a>) -> SexpResult<Sexp<'a>> {
    bitwise_op(a, b, |x, y| x ^ y, b"bitwXor\0")
}

/// Generic bitwise operation with type coercion and recycling.
fn bitwise_op<'a, F: Fn(c_int, c_int) -> c_int>(
    a: &Sexp<'a>,
    b: &Sexp<'a>,
    op: F,
    name: &'static [u8],
) -> SexpResult<Sexp<'a>> {
    let a = coerce_bitwise_operand(a)?;
    let b = coerce_bitwise_operand(b)?;

    if a.typeof_() != b.typeof_() {
        bitwise_type_mismatch();
    }

    match a.typeof_() {
        SEXPTYPE::INTSXP => {
            let m = a.len();
            let n = b.len();
            let mn = if m > 0 && n > 0 { m.max(n) } else { 0 };
            let mut ans = alloc_in(&a, SEXPTYPE::INTSXP, mn)?;
            mod_iterate2_try(mn, m, n, |i, ia, ib| {
                let aa = a.try_integer_elt(ia)?;
                let bb = b.try_integer_elt(ib)?;
                ans.try_set_integer_elt(
                    i,
                    if is_na_int(aa) || is_na_int(bb) {
                        NA_INTEGER
                    } else {
                        op(aa, bb)
                    },
                )
            })?;
            Ok(ans.freeze())
        }
        _ => UNIMPLEMENTED_TYPE(name.as_ptr() as *const c_char, a.as_raw()),
    }
}

/// Coerce the shift-amount operand: anything non-integer goes to INTSXP.
fn bitwise_shift_rhs<'a>(x: &Sexp<'a>) -> SexpResult<Sexp<'a>> {
    if x.typeof_() != SEXPTYPE::INTSXP {
        coerce_to_int(x)
    } else {
        Ok(x.clone())
    }
}

/// Bitwise left shift of integer vectors with recycling.
fn bitwise_shift_l<'a>(a: &Sexp<'a>, b: &Sexp<'a>) -> SexpResult<Sexp<'a>> {
    bitwise_shift(
        a,
        b,
        |aa, bb| ((aa as c_uint) << bb) as c_int,
        b"bitShiftL\0",
    )
}

/// Bitwise right shift of integer vectors with recycling.
fn bitwise_shift_r<'a>(a: &Sexp<'a>, b: &Sexp<'a>) -> SexpResult<Sexp<'a>> {
    bitwise_shift(
        a,
        b,
        |aa, bb| ((aa as c_uint) >> bb) as c_int,
        b"bitShiftR\0",
    )
}

/// Shared shift kernel: NA or out-of-range counts yield NA_INTEGER.
fn bitwise_shift<'a, F: Fn(c_int, c_int) -> c_int>(
    a: &Sexp<'a>,
    b: &Sexp<'a>,
    op: F,
    name: &'static [u8],
) -> SexpResult<Sexp<'a>> {
    let a = coerce_bitwise_operand(a)?;
    let b = bitwise_shift_rhs(b)?;

    if a.typeof_() != b.typeof_() {
        bitwise_type_mismatch();
    }

    match a.typeof_() {
        SEXPTYPE::INTSXP => {
            let m = a.len();
            let n = b.len();
            let mn = if m > 0 && n > 0 { m.max(n) } else { 0 };
            let mut ans = alloc_in(&a, SEXPTYPE::INTSXP, mn)?;
            mod_iterate2_try(mn, m, n, |i, ia, ib| {
                let aa = a.try_integer_elt(ia)?;
                let bb = b.try_integer_elt(ib)?;
                ans.try_set_integer_elt(
                    i,
                    if is_na_int(aa) || is_na_int(bb) || bb < 0 || bb > 31 {
                        NA_INTEGER
                    } else {
                        op(aa, bb)
                    },
                )
            })?;
            Ok(ans.freeze())
        }
        _ => UNIMPLEMENTED_TYPE(name.as_ptr() as *const c_char, a.as_raw()),
    }
}

// ---------------------------------------------------------------------------
// do_bitwise -- entry point for bitwise operators
// ---------------------------------------------------------------------------

/// Entry point for bitwise operators dispatched from R's internal mechanism.
///
/// # Safety
/// ABI boundary registered via `PrimFun`; the body delegates to checked
/// kernels through the session wrap in [`checked`].
pub fn do_bitwise(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, env);
    let a = checked(CAR(args));
    let b = checked(CADR(args));
    match primitive_name(op).as_str() {
        "bitwAnd" => unwrap_sexp(bitwise_and(&a, &b)),
        "bitwNot" => unwrap_sexp(bitwise_not(&a)),
        "bitwOr" => unwrap_sexp(bitwise_or(&a, &b)),
        "bitwXor" => unwrap_sexp(bitwise_xor(&a, &b)),
        "bitwShiftL" => unwrap_sexp(bitwise_shift_l(&a, &b)),
        "bitwShiftR" => unwrap_sexp(bitwise_shift_r(&a, &b)),
        _ => match PRIMVAL(op) {
            BITWISE_AND => unwrap_sexp(bitwise_and(&a, &b)),
            BITWISE_NOT => unwrap_sexp(bitwise_not(&a)),
            BITWISE_OR => unwrap_sexp(bitwise_or(&a, &b)),
            BITWISE_XOR => unwrap_sexp(bitwise_xor(&a, &b)),
            BITWISE_SHIFT_L => unwrap_sexp(bitwise_shift_l(&a, &b)),
            BITWISE_SHIFT_R => unwrap_sexp(bitwise_shift_r(&a, &b)),
            _ => ptr::null_mut(),
        },
    }
}

pub fn do_bitwAnd(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unwrap_sexp(bitwise_and(&checked(CAR(args)), &checked(CADR(args))))
}

pub fn do_bitwOr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unwrap_sexp(bitwise_or(&checked(CAR(args)), &checked(CADR(args))))
}

pub fn do_bitwXor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unwrap_sexp(bitwise_xor(&checked(CAR(args)), &checked(CADR(args))))
}

pub fn do_bitwNot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unwrap_sexp(bitwise_not(&checked(CAR(args))))
}

pub fn do_bitwShiftL(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unwrap_sexp(bitwise_shift_l(&checked(CAR(args)), &checked(CADR(args))))
}

pub fn do_bitwShiftR(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unwrap_sexp(bitwise_shift_r(&checked(CAR(args)), &checked(CADR(args))))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::accessors::*;
    use crate::sexp::constructors::Rf_cons;
    use crate::sexp::ffi::Rcomplex;

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
        {
            assert!(!is_scalar_string(ptr::null_mut()));
        }
    }

    #[test]
    fn test_is_scalar_string_non_string() {
        let _session = crate::sexp::session::RSession::new();
        let v = Rf_allocVector(SEXPTYPE::INTSXP, 1);
        assert!(!is_scalar_string(v));
    }

    #[test]
    fn test_is_scalar_string_correct() {
        let _session = crate::sexp::session::RSession::new();
        let v = Rf_allocVector(SEXPTYPE::STRSXP, 1);
        assert!(is_scalar_string(v));
    }

    #[test]
    fn test_is_scalar_string_wrong_length() {
        let _session = crate::sexp::session::RSession::new();
        let v = Rf_allocVector(SEXPTYPE::STRSXP, 3);
        assert!(!is_scalar_string(v));
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

            let ans = numeric_relop(EQOP, &checked(s1), &checked(s2))
                .unwrap()
                .as_raw();
            assert!(!ans.is_null());
            assert_eq!(*LOGICAL(ans).add(0), 1); // 1 == 1
            assert_eq!(*LOGICAL(ans).add(1), 0); // 2 != 4
            assert_eq!(*LOGICAL(ans).add(2), 1); // 3 == 3

            let ans_lt = numeric_relop(LTOP, &checked(s1), &checked(s2))
                .unwrap()
                .as_raw();
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

            let ans = numeric_relop(EQOP, &checked(s1), &checked(s2))
                .unwrap()
                .as_raw();
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

            let ans = numeric_relop(EQOP, &checked(s1), &checked(s2))
                .unwrap()
                .as_raw();
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

            let ans = numeric_relop(NEOP, &checked(s1), &checked(s2))
                .unwrap()
                .as_raw();
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

            let ans = numeric_relop(EQOP, &checked(s1), &checked(s2))
                .unwrap()
                .as_raw();
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

            let ans = raw_relop(EQOP, &checked(s1), &checked(s2))
                .unwrap()
                .as_raw();
            assert_eq!(*LOGICAL(ans).add(0), 1);
            assert_eq!(*LOGICAL(ans).add(1), 0);
            assert_eq!(*LOGICAL(ans).add(2), 1);

            let ans_lt = raw_relop(LTOP, &checked(s1), &checked(s2))
                .unwrap()
                .as_raw();
            assert_eq!(*LOGICAL(ans_lt).add(0), 0);
            assert_eq!(*LOGICAL(ans_lt).add(1), 1);
            assert_eq!(*LOGICAL(ans_lt).add(2), 0);
        }
    }

    #[test]
    fn test_check_arity_rejects_mismatched_primitive_argument_count() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let op = crate::mainutils::names::R_Primitive(c"length".as_ptr());
            assert_eq!(TYPEOF(op), SEXPTYPE::BUILTINSXP);

            let one_arg = Rf_cons(R_NilValue(), R_NilValue());
            checkArity(op, one_arg);

            let two_args = Rf_cons(R_NilValue(), Rf_cons(R_NilValue(), R_NilValue()));
            let err = std::panic::catch_unwind(|| {
                checkArity(op, two_args);
            });
            assert!(err.is_err());
            let payload = err.unwrap_err();
            let r_error = payload
                .downcast_ref::<crate::sexp::context::RError>()
                .expect("arity mismatch should raise RError");
            assert_eq!(
                r_error.message,
                "2 arguments passed to 'length' which requires 1"
            );
        }
    }

    #[test]
    fn test_bitwise_unsupported_type_errors() {
        let _session = crate::sexp::session::RSession::new();
        let string = Rf_allocVector(SEXPTYPE::STRSXP, 1);
        let err = std::panic::catch_unwind(|| {
            let _ = bitwise_not(&checked(string));
        })
        .expect_err("unsupported bitwise type should raise an RError");
        let message = err
            .downcast_ref::<crate::sexp::context::RError>()
            .map(|err| err.message.as_str())
            .unwrap_or("");
        assert!(message.contains("bitwNot: unsupported SEXPTYPE"));
    }

    #[test]
    fn test_bitwise_mismatched_types_error_instead_of_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let integer = Rf_allocVector(SEXPTYPE::INTSXP, 1);
            let logical = Rf_allocVector(SEXPTYPE::LGLSXP, 1);
            *INTEGER(integer) = 1;
            *LOGICAL(logical) = 1;

            let err = std::panic::catch_unwind(|| {
                let _ = bitwise_op(
                    &checked(integer),
                    &checked(logical),
                    |x, y| x & y,
                    b"bitwAnd\0",
                );
            })
            .expect_err("mismatched bitwise operands should raise an RError");
            let message = err
                .downcast_ref::<crate::sexp::context::RError>()
                .map(|err| err.message.as_str())
                .unwrap_or("");
            assert_eq!(message, "'a' and 'b' must have the same type");
        }
    }

    #[test]
    fn test_relop_wrong_argument_count_errors() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let op = crate::mainutils::names::R_Primitive(c"==".as_ptr());
            let args = Rf_cons(Rf_ScalarLogical(1), R_NilValue());
            let err = std::panic::catch_unwind(|| {
                let _ = do_relop(ptr::null_mut(), op, args, R_NilValue());
            })
            .expect_err("relop should require two arguments");
            let message = err
                .downcast_ref::<crate::sexp::context::RError>()
                .map(|err| err.message.as_str())
                .unwrap_or("");
            assert_eq!(message, "operator needs two arguments");
        }
    }

    #[test]
    fn test_relop_language_ordering_errors() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let op = crate::mainutils::names::R_Primitive(c"<".as_ptr());
            let x = crate::sexp::symbol::Rf_install(c"x".as_ptr());
            let y = crate::sexp::symbol::Rf_install(c"y".as_ptr());
            let err = std::panic::catch_unwind(|| {
                let _ = do_relop_dflt(ptr::null_mut(), op, x, y);
            })
            .expect_err("ordering language objects should raise an RError");
            let message = err
                .downcast_ref::<crate::sexp::context::RError>()
                .map(|err| err.message.as_str())
                .unwrap_or("");
            assert!(message.contains("comparison (<) is not possible for language types"));
        }
    }

    #[test]
    fn test_relop_non_vector_operands_error_instead_of_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let op = crate::mainutils::names::R_Primitive(c"==".as_ptr());
            let x = crate::sexp::memory_ext::allocSExp(SEXPTYPE::EXTPTRSXP);
            let y = crate::sexp::memory_ext::allocSExp(SEXPTYPE::EXTPTRSXP);
            let err = std::panic::catch_unwind(|| {
                let _ = do_relop_dflt(ptr::null_mut(), op, x, y);
            })
            .expect_err("non-vector relop operands should raise an RError");
            let message = err
                .downcast_ref::<crate::sexp::context::RError>()
                .map(|err| err.message.as_str())
                .unwrap_or("");
            assert!(message.contains("comparison (==) is possible only for atomic and list types"));
        }
    }

    #[test]
    fn test_complex_ordering_errors_instead_of_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = Rf_allocVector(SEXPTYPE::CPLXSXP, 1);
            let y = Rf_allocVector(SEXPTYPE::CPLXSXP, 1);
            *COMPLEX(x) = Rcomplex { r: 1.0, i: 0.0 };
            *COMPLEX(y) = Rcomplex { r: 2.0, i: 0.0 };
            let err = std::panic::catch_unwind(|| {
                let _ = complex_relop(LTOP, &checked(x), &checked(y));
            })
            .expect_err("complex ordering should raise an RError");
            let message = err
                .downcast_ref::<crate::sexp::context::RError>()
                .map(|err| err.message.as_str())
                .unwrap_or("");
            assert_eq!(message, "invalid comparison with complex values");
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
