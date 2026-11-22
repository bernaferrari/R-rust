#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/apply.c -- apply functions (lapply, vapply, rapply, islistfactor).
//!
//! Original C source: R-4.4.x src/main/apply.c (417 lines).
//!
//! Implements the .Internal primitives behind R's `lapply()`, `vapply()`,
//! `rapply()`, and `islistfactor()` functions.
//!
//! Now uses the real evaluator (Rf_eval), closure application (applyClosure),
//! environment operations (defineVar, R_findVarInFrame, forcePromise), and
//! attribute operations (getAttrib, setAttrib).

use std::os::raw::{c_double, c_int};
#[cfg(test)]
use std::ptr;

use crate::sexp::accessors::{
    CADDDR, CADDR, CADR, CAR, CDR, INTEGER_ELT, LENGTH, LOGICAL_ELT, NAMED, Rf_isNull,
    SET_INTEGER_ELT, SET_NAMED, SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, STRING_ELT, TYPEOF,
    VECTOR_ELT, XLENGTH, char_cstr_bytes_opt,
};
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::eval::attrib_core::{
    R_ClassSymbol, R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, R_data_class, getAttrib, setAttrib,
};
use crate::eval::eval::Rf_eval;
use crate::mainutils::duplicate::{duplicate, lazy_duplicate, shallow_duplicate};
use crate::mainutils::essentials::{
    complex_elt_checked, integer_elt_checked, logical_elt_checked, raw_elt_checked,
    real_elt_checked, set_complex_checked, set_integer_checked, set_logical_checked,
    set_raw_checked, set_real_checked,
};
use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector, Rf_allocVector3, Rf_cons, Rf_lang3};
use crate::sexp::envir::{R_findVarInFrame, defineVar, forcePromise};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{protect, protect_with_index_raw};

// ---------------------------------------------------------------------------
// SEXPTYPE constants used in this module
// ---------------------------------------------------------------------------

const VECSXP_VAL: SEXPTYPE = SEXPTYPE::VECSXP;
const EXPRSXP_VAL: SEXPTYPE = SEXPTYPE::EXPRSXP;
const LGLSXP_VAL: SEXPTYPE = SEXPTYPE::LGLSXP;
const INTSXP_VAL: SEXPTYPE = SEXPTYPE::INTSXP;
const REALSXP_VAL: SEXPTYPE = SEXPTYPE::REALSXP;
const CPLXSXP_VAL: SEXPTYPE = SEXPTYPE::CPLXSXP;
const STRSXP_VAL: SEXPTYPE = SEXPTYPE::STRSXP;
const RAWSXP_VAL: SEXPTYPE = SEXPTYPE::RAWSXP;
const SYMSXP_VAL: SEXPTYPE = SEXPTYPE::SYMSXP;
const CLOSXP_VAL: SEXPTYPE = SEXPTYPE::CLOSXP;
const SPECIALSXP_VAL: SEXPTYPE = SEXPTYPE::SPECIALSXP;
const BUILTINSXP_VAL: SEXPTYPE = SEXPTYPE::BUILTINSXP;
const NILSXP_VAL: SEXPTYPE = SEXPTYPE::NILSXP;

// ---------------------------------------------------------------------------
// Local helper functions (matching R's internal macros/utilities)
// ---------------------------------------------------------------------------

/// Check arity -- no-op since the R-level wrapper handles this.
fn checkArity(op: SEXP, args: SEXP) {
    crate::mainutils::relop::checkArity(op, args)
}

/// Coerce to logical scalar.
fn asLogical(x: SEXP) -> c_int {
    if x.is_null() || x == R_NilValue() {
        return NA_LOGICAL;
    }
    match TYPEOF(x) {
        t if t == LGLSXP_VAL => {
            if LENGTH(x) == 0 {
                NA_LOGICAL
            } else {
                LOGICAL_ELT(x, 0)
            }
        }
        t if t == INTSXP_VAL => {
            if LENGTH(x) == 0 {
                NA_LOGICAL
            } else {
                INTEGER_ELT(x, 0)
            }
        }
        _ => 0,
    }
}

/// Coerce to boolean (TRUE/FALSE only, error on NA).
fn asBool2(x: SEXP, _call: SEXP) -> bool {
    let v = asLogical(x);
    if v == NA_LOGICAL {
        eprintln!("Error: invalid 'x' value in asBool2");
        std::panic::panic_any(crate::sexp::context::RError {
            message: "invalid 'x' value".to_string(),
        });
    }
    v != 0
}

/// Check if type is VECSXP or EXPRSXP.
fn isVectorList(x: SEXP) -> c_int {
    {
        if x.is_null() || x == R_NilValue() {
            return 0;
        }
        let t = TYPEOF(x);
        (t == VECSXP_VAL || t == EXPRSXP_VAL) as c_int
    }
}

/// Check if type is CLOSXP, BUILTINSXP, or SPECIALSXP.
fn isFunction(x: SEXP) -> c_int {
    {
        if x.is_null() || x == R_NilValue() {
            return 0;
        }
        let t = TYPEOF(x);
        (t == CLOSXP_VAL || t == SPECIALSXP_VAL || t == BUILTINSXP_VAL) as c_int
    }
}

/// Check if type is STRSXP.
fn isString(x: SEXP) -> c_int {
    {
        if x.is_null() || x == R_NilValue() {
            return 0;
        }
        (TYPEOF(x) == STRSXP_VAL) as c_int
    }
}

/// Check if x is any atomic or generic vector type.
fn isVector(x: SEXP) -> c_int {
    {
        if x.is_null() || x == R_NilValue() {
            return 0;
        }
        let t = TYPEOF(x);
        (t == LGLSXP_VAL
            || t == INTSXP_VAL
            || t == REALSXP_VAL
            || t == CPLXSXP_VAL
            || t == STRSXP_VAL
            || t == VECSXP_VAL
            || t == EXPRSXP_VAL
            || t == RAWSXP_VAL) as c_int
    }
}

/// MARK_NOT_MUTABLE: set the namedness to NAMEDMAX (2) to prevent modification.
fn MARK_NOT_MUTABLE(x: SEXP) {
    if !x.is_null() {
        SET_NAMED(x, 2);
    }
}

/// INCREMENT_NAMED: increase namedness level by 1, capped at 2.
fn INCREMENT_NAMED(x: SEXP) {
    if !x.is_null() {
        let n = NAMED(x);
        if n < 2 {
            SET_NAMED(x, n + 1);
        }
    }
}

/// MAYBE_REFERENCED: true if namedness > 0 (potentially shared).
fn MAYBE_REFERENCED(x: SEXP) -> bool {
    NAMED(x) > 0
}

/// MAYBE_SHARED: true if namedness >= 2 (definitely shared).
fn MAYBE_SHARED(x: SEXP) -> bool {
    NAMED(x) >= 2
}

/// Seql: check if two CHARSXP strings are equal (pointer equality for
/// interned strings is sufficient for ASCII comparison).
fn Seql(a: SEXP, b: SEXP) -> c_int {
    if a == b {
        return 1;
    }
    // Fall back to byte comparison
    if a.is_null() || b.is_null() {
        return 0;
    }
    match (char_cstr_bytes_opt(a), char_cstr_bytes_opt(b)) {
        (Some(a), Some(b)) => (a == b) as c_int,
        _ => 0,
    }
}

/// isFactor: check if x has class "factor" or is an ordered factor.
pub fn isFactor(x: SEXP) -> c_int {
    if x.is_null() || x == R_NilValue() {
        return 0;
    }
    let klass = getAttrib(x, R_ClassSymbol());
    if klass.is_null() || klass == R_NilValue() {
        // No class attribute; check for "levels" attribute (old-style factor)
        let levels_sym = crate::sexp::symbol::install_str("levels");
        let levels = getAttrib(x, levels_sym);
        if !levels.is_null() && levels != R_NilValue() {
            return 1;
        }
        return 0;
    }
    if TYPEOF(klass) != STRSXP_VAL || LENGTH(klass) == 0 {
        return 0;
    }
    // Check if any element of class is "factor" or "ordered"
    let n = LENGTH(klass);
    for i in 0..n {
        if let Some(bytes) = char_cstr_bytes_opt(STRING_ELT(klass, i as R_xlen_t))
            && matches!(bytes.as_slice(), b"factor" | b"ordered")
        {
            return 1;
        }
    }
    0
}

/// R_typeToChar: convert SEXPTYPE to its string name.
fn R_typeToChar_local(x: SEXP) -> &'static str {
    if x.is_null() {
        return "NULL";
    }
    match TYPEOF(x) {
        0 => "NULL",
        1 => "symbol",
        2 => "pairlist",
        3 => "closure",
        4 => "environment",
        5 => "promise",
        6 => "language",
        7 => "special",
        8 => "builtin",
        9 => "character",
        10 => "logical",
        13 => "integer",
        14 => "double",
        15 => "complex",
        16 => "character",
        17 => "...",
        18 => "any",
        19 => "list",
        20 => "expression",
        21 => "bytecode",
        22 => "externalptr",
        23 => "weakref",
        24 => "raw",
        25 => "S4",
        _ => "unknown",
    }
}

/// LCONS: create a cons cell with LANGSXP type (LCONS in R's C API).
fn LCONS(car: SEXP, cdr: SEXP) -> SEXP {
    let cell = Rf_cons(car, cdr);
    if !cell.is_null() {
        crate::sexp::accessors::SET_TYPEOF(cell, SEXPTYPE::LANGSXP.as_c_int());
    }
    cell
}

/// Get the length of an SEXP as c_int (handles pairlists too).
fn length(x: SEXP) -> c_int {
    {
        if x.is_null() || x == R_NilValue() {
            return 0;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP {
            // Count pairlist elements
            let mut count: c_int = 0;
            let mut cur = x;
            while !cur.is_null() && cur != R_NilValue() {
                count += 1;
                cur = CDR(cur);
            }
            count
        } else {
            LENGTH(x)
        }
    }
}

/// force_and_call: equivalent of R's C `R_forceAndCall(e, n, rho)`.
///
/// Takes a LANGSXP expression `e`, forces the first `n` promises in its
/// argument list, then evaluates the function call.
fn force_and_call(e: SEXP, n: c_int, rho: SEXP) -> SEXP {
    if e.is_null() || TYPEOF(e) != SEXPTYPE::LANGSXP {
        return Rf_eval(e, rho);
    }

    // Force the first n promises in CDR(e)
    let mut count: c_int = 0;
    let mut current = CDR(e);
    while !current.is_null() && current != R_NilValue() && count < n {
        let val = CAR(current);
        if TYPEOF(val) == SEXPTYPE::PROMSXP {
            let forced_val = forcePromise(val);
            SETCAR(current, forced_val);
        }
        count += 1;
        current = CDR(current);
    }

    // Evaluate the call
    Rf_eval(e, rho)
}

/// error helper: print message and panic with RError.
fn r_error(msg: &str) -> ! {
    eprintln!("Error: {}", msg);
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

/// error helper with format string.
macro_rules! r_error {
    ($($arg:tt)*) => {{
        eprintln!("Error: {}", format!($($arg)*));
        std::panic::panic_any(crate::sexp::context::RError {
            message: format!($($arg)*),
        });
    }};
}

fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::mainutils::coerce::coerceVector(x, type_)
}

// ---------------------------------------------------------------------------
// checkArgIsSymbol: validate that the given SEXP is a symbol
// ---------------------------------------------------------------------------

fn checkArgIsSymbol(x: SEXP) -> SEXP {
    {
        if x.is_null() || TYPEOF(x) != SYMSXP_VAL {
            r_error("argument must be a symbol");
        }
        x
    }
}

// ---------------------------------------------------------------------------
// do_lapply -- .Internal(lapply(X, FUN))
//
// This is a special .Internal with unevaluated arguments. It is called from
// a closure wrapper, so X and FUN are symbols bound to promises in rho.
// ---------------------------------------------------------------------------

pub fn do_lapply(_call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    checkArity(op, args);

    // X and FUN are symbols bound to promises in rho
    let X = checkArgIsSymbol(CAR(args));
    let XX = Rf_eval(CAR(args), rho);
    let _xx_guard = protect(XX);
    let n: R_xlen_t = XLENGTH(XX);
    let FUN = checkArgIsSymbol(CADR(args));
    let realIndx: bool = n > c_int::MAX as R_xlen_t;

    // Allocate result vector
    let ans = Rf_allocVector3(VECSXP_VAL, n);
    let _ans_guard = protect(ans);
    let names = getAttrib(XX, R_NamesSymbol());
    if Rf_isNull(names) == 0 {
        setAttrib(ans, R_NamesSymbol(), names);
    }

    // Build call: FUN(X[[<ind>]], ...)
    let isym = crate::sexp::symbol::install_str("i");
    let tmp = Rf_lang3(crate::sexp::symbol::R_Bracket2Symbol(), X, isym);
    let _tmp_guard = protect(tmp);
    let R_fcall = Rf_lang3(FUN, tmp, crate::sexp::symbol::R_DotsSymbol());
    let _fcall_guard = protect(R_fcall);
    MARK_NOT_MUTABLE(R_fcall);

    // Create the loop index variable and value
    let mut ind = Rf_allocVector(if realIndx { REALSXP_VAL } else { INTSXP_VAL }, 1);
    let mut ind_guard = protect_with_index_raw(ind, "do_lapply ind");
    defineVar(isym, ind, rho);
    INCREMENT_NAMED(ind);

    // Main loop
    for i in 0..n {
        if realIndx {
            set_real_checked(ind, 0, (i + 1) as c_double);
        } else {
            SET_INTEGER_ELT(ind, 0, (i + 1) as c_int);
        }

        let mut tmp2 = force_and_call(R_fcall, 1, rho);
        if MAYBE_REFERENCED(tmp2) {
            tmp2 = lazy_duplicate(tmp2);
        }
        SET_VECTOR_ELT(ans, i, tmp2);

        // Check if ind was captured or removed by FUN
        let cur_val = R_findVarInFrame(rho, isym);
        if cur_val != ind || MAYBE_SHARED(ind) {
            ind = duplicate(ind);
            ind_guard.reprotect_raw(ind);
            defineVar(isym, ind, rho);
            INCREMENT_NAMED(ind);
        }
    }

    ans
}

// ---------------------------------------------------------------------------
// do_vapply -- .Internal(vapply(X, FUN, FUN.VALUE, USE.NAMES))
//
// This is a special .Internal with unevaluated arguments.
// ---------------------------------------------------------------------------

pub fn do_vapply(_call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    checkArity(op, args);

    let X = CAR(args);
    let _x_guard = protect(X);
    let XX = Rf_eval(CAR(args), rho);
    let _xx_guard = protect(XX);
    let FUN = CADR(args); // must be unevaluated for use in e.g. bquote
    let value = Rf_eval(CADDR(args), rho);
    let _value_guard = protect(value);

    if isVector(value) == 0 {
        r_error("'FUN.VALUE' must be a vector");
    }

    let use_names_val = Rf_eval(CADDDR(args), rho);
    let _use_names_guard = protect(use_names_val);
    let useNames = asLogical(use_names_val);

    if useNames == NA_LOGICAL {
        r_error("invalid 'USE.NAMES' value");
    }

    let n: R_xlen_t = XLENGTH(XX);
    if n == NA_INTEGER as R_xlen_t {
        r_error("invalid length");
    }
    let realIndx: bool = n > c_int::MAX as R_xlen_t;

    let commonLen: c_int = length(value);
    if commonLen > 1 && n > c_int::MAX as R_xlen_t {
        r_error("long vectors are not supported for matrix/array results");
    }
    let commonType: c_int = TYPEOF(value);

    // Check supported type
    if commonType != CPLXSXP_VAL
        && commonType != REALSXP_VAL
        && commonType != INTSXP_VAL
        && commonType != LGLSXP_VAL
        && commonType != RAWSXP_VAL
        && commonType != STRSXP_VAL
        && commonType != VECSXP_VAL
    {
        r_error!("type '{}' is not supported", R_typeToChar_local(value));
    }

    let dim_v = getAttrib(value, R_DimSymbol());
    let array_value: bool = TYPEOF(dim_v) == INTSXP_VAL && LENGTH(dim_v) >= 1;

    let result_len = n
        .checked_mul(R_xlen_t::from(commonLen))
        .unwrap_or_else(|| r_error("result is too long"));
    let ans = Rf_allocVector3(commonType, result_len);
    let _ans_guard = protect(ans);

    let mut names: SEXP = R_NilValue();
    let mut rowNames: SEXP = R_NilValue();
    let mut rowNames_guard = None;

    if useNames != 0 {
        names = getAttrib(XX, R_NamesSymbol());
        if Rf_isNull(names) != 0 && TYPEOF(XX) == STRSXP_VAL {
            names = XX;
        }
        rowNames = getAttrib(
            value,
            if array_value {
                R_DimNamesSymbol()
            } else {
                R_NamesSymbol()
            },
        );
        rowNames_guard = Some(protect_with_index_raw(rowNames, "do_vapply rowNames"));
    }

    // Build call: FUN(XX[[<ind>]], ...)
    let isym = crate::sexp::symbol::install_str("i");
    let mut ind = Rf_allocVector(if realIndx { REALSXP_VAL } else { INTSXP_VAL }, 1);
    let mut ind_guard = protect_with_index_raw(ind, "do_vapply ind");
    defineVar(isym, ind, rho);
    INCREMENT_NAMED(ind);

    let tmp = LCONS(
        crate::sexp::symbol::R_Bracket2Symbol(),
        LCONS(X, LCONS(isym, R_NilValue())),
    );
    let _tmp_guard = protect(tmp);
    let R_fcall = LCONS(
        FUN,
        LCONS(tmp, LCONS(crate::sexp::symbol::R_DotsSymbol(), R_NilValue())),
    );
    let _fcall_guard = protect(R_fcall);

    for i in 0..n {
        if realIndx {
            set_real_checked(ind, 0, (i + 1) as c_double);
        } else {
            SET_INTEGER_ELT(ind, 0, (i + 1) as c_int);
        }

        let mut val = force_and_call(R_fcall, 1, rho);
        if MAYBE_REFERENCED(val) {
            val = lazy_duplicate(val);
        }
        let mut val_guard = protect_with_index_raw(val, "do_vapply val");

        if length(val) != commonLen {
            r_error!(
                "values must be length {}, but FUN(X[[{}]]) result is length {}",
                commonLen,
                i + 1,
                length(val),
            );
        }

        let valType = TYPEOF(val);
        if valType != commonType {
            let mut okay = false;
            match commonType {
                t if t == CPLXSXP_VAL => {
                    okay = valType == REALSXP_VAL || valType == INTSXP_VAL || valType == LGLSXP_VAL;
                }
                t if t == REALSXP_VAL => {
                    okay = valType == INTSXP_VAL || valType == LGLSXP_VAL;
                }
                t if t == INTSXP_VAL => {
                    okay = valType == LGLSXP_VAL;
                }
                _ => {} // intentionally unhandled: type mismatch caught by !okay check below
            }
            if !okay {
                r_error!(
                    "values must be type '{}', but FUN(X[[{}]]) result is type '{}'",
                    R_typeToChar_local(value),
                    i + 1,
                    R_typeToChar_local(val),
                );
            }
            val = coerceVector(val, commonType);
            val_guard.reprotect_raw(val);
        }

        // Take row names from the first result only
        if i == 0 && useNames != 0 && Rf_isNull(rowNames) != 0 {
            rowNames = getAttrib(
                val,
                if array_value {
                    R_DimNamesSymbol()
                } else {
                    R_NamesSymbol()
                },
            );
            if let Some(guard) = rowNames_guard.as_mut() {
                guard.reprotect_raw(rowNames);
            }
        }

        // Copy values into ans
        let start = i * R_xlen_t::from(commonLen);
        for j in 0..R_xlen_t::from(commonLen) {
            let dst = start + j;
            match commonType {
                t if t == CPLXSXP_VAL => {
                    set_complex_checked(ans, dst, complex_elt_checked(val, j))
                }
                t if t == REALSXP_VAL => set_real_checked(ans, dst, real_elt_checked(val, j)),
                t if t == INTSXP_VAL => {
                    set_integer_checked(ans, dst, integer_elt_checked(val, j))
                }
                t if t == LGLSXP_VAL => {
                    set_logical_checked(ans, dst, logical_elt_checked(val, j))
                }
                t if t == RAWSXP_VAL => set_raw_checked(ans, dst, raw_elt_checked(val, j)),
                t if t == STRSXP_VAL => SET_STRING_ELT(ans, dst, STRING_ELT(val, j)),
                t if t == VECSXP_VAL => SET_VECTOR_ELT(ans, dst, VECTOR_ELT(val, j)),
                _ => unreachable!("vapply validated the result type"),
            }
        }

        let cur_val = R_findVarInFrame(rho, isym);
        if cur_val != ind || MAYBE_SHARED(ind) {
            ind = duplicate(ind);
            ind_guard.reprotect_raw(ind);
            defineVar(isym, ind, rho);
            INCREMENT_NAMED(ind);
        }
    }

    // Set dim attribute if commonLen != 1
    if commonLen != 1 {
        let rnk_v: c_int = if array_value { LENGTH(dim_v) } else { 1 };
        let dim = Rf_allocVector(INTSXP_VAL, rnk_v + 1);
        let _dim_guard = protect(dim);
        if array_value {
            for j in 0..rnk_v {
                SET_INTEGER_ELT(dim, j, INTEGER_ELT(dim_v, j));
            }
        } else {
            SET_INTEGER_ELT(dim, 0, commonLen);
        }
        SET_INTEGER_ELT(dim, rnk_v, n as c_int);
        setAttrib(ans, R_DimSymbol(), dim);

        // Set dimnames if useNames
        if useNames != 0 {
            if Rf_isNull(names) == 0 || Rf_isNull(rowNames) == 0 {
                let dimnames = Rf_allocVector(VECSXP_VAL, rnk_v + 1);
                let _dimnames_guard = protect(dimnames);
                if array_value && Rf_isNull(rowNames) == 0 {
                    if TYPEOF(rowNames) != VECSXP_VAL || LENGTH(rowNames) != rnk_v {
                        r_error!(
                            "dimnames(<value>) is neither NULL nor list of length {}",
                            rnk_v,
                        );
                    }
                    for j in 0..rnk_v {
                        SET_VECTOR_ELT(
                            dimnames,
                            j as R_xlen_t,
                            VECTOR_ELT(rowNames, j as R_xlen_t),
                        );
                    }
                } else {
                    SET_VECTOR_ELT(dimnames, 0, rowNames);
                }
                SET_VECTOR_ELT(dimnames, rnk_v as R_xlen_t, names);
                setAttrib(ans, R_DimNamesSymbol(), dimnames);
            }
        }
    } else if useNames != 0 {
        // commonLen == 1: just set names
        if Rf_isNull(names) == 0 {
            setAttrib(ans, R_NamesSymbol(), names);
        }
    }

    ans
}

// ---------------------------------------------------------------------------
// do_one -- recursive workhorse for do_rapply
//
// Apply FUN() to X recursively.
// ---------------------------------------------------------------------------

pub(crate) fn do_one(
    x: SEXP,
    fun: SEXP,
    classes: SEXP,
    deflt: SEXP,
    replace: bool,
    rho: SEXP,
) -> SEXP {
    // If X is a list (or NULL), recurse
    if x.is_null() || x == R_NilValue() || isVectorList(x) != 0 {
        let n: R_xlen_t = if x.is_null() || x == R_NilValue() {
            0
        } else {
            XLENGTH(x)
        };
        let ans = if replace {
            shallow_duplicate(x)
        } else {
            let a = Rf_allocVector(VECSXP_VAL, n as c_int);
            let names = getAttrib(x, R_NamesSymbol());
            if Rf_isNull(names) == 0 {
                setAttrib(a, R_NamesSymbol(), names);
            }
            a
        };
        let _ans_guard = protect(ans);

        for i in 0..n {
            let elt = VECTOR_ELT(x, i);
            let result = do_one(elt, fun, classes, deflt, replace, rho);
            SET_VECTOR_ELT(ans, i, result);
        }

        return ans;
    }

    // Check if X matches any of the classes
    let mut matched = false;
    if isString(classes) != 0 && LENGTH(classes) > 0 {
        matched = char_cstr_bytes_opt(STRING_ELT(classes, 0)).is_some_and(|name| name == b"ANY");
    }

    if !matched {
        let klass = R_data_class(x);
        let _klass_guard = protect(klass);
        let nklass = if TYPEOF(klass) == STRSXP_VAL {
            LENGTH(klass)
        } else {
            0
        };
        let nclasses = if isString(classes) != 0 {
            LENGTH(classes)
        } else {
            0
        };

        for i in 0..nklass {
            for j in 0..nclasses {
                if Seql(
                    STRING_ELT(klass, i as R_xlen_t),
                    STRING_ELT(classes, j as R_xlen_t),
                ) != 0
                {
                    matched = true;
                    break;
                }
            }
            if matched {
                break;
            }
        }
    }

    if matched {
        // Build and evaluate call: FUN(X, ...)
        let Xsym = crate::sexp::symbol::install_str("X");
        defineVar(Xsym, x, rho);
        INCREMENT_NAMED(x);

        let R_fcall = Rf_lang3(fun, Xsym, crate::sexp::symbol::R_DotsSymbol());
        let _fcall_guard = protect(R_fcall);
        let mut ans = force_and_call(R_fcall, 1, rho);
        if MAYBE_REFERENCED(ans) {
            ans = lazy_duplicate(ans);
        }
        ans
    } else if replace {
        lazy_duplicate(x)
    } else {
        lazy_duplicate(deflt)
    }
}

// ---------------------------------------------------------------------------
// do_rapply -- .Internal(rapply(object, f, classes, how, ...))
//
// Recursively applies FUN to non-list elements of X that match the given
// classes. If `how` is "replace", matched elements are replaced in-place;
// otherwise a new list is returned.
// ---------------------------------------------------------------------------

pub fn do_rapply(_call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    checkArity(op, args);

    let mut args_remaining = args;
    let x = CAR(args_remaining);
    args_remaining = CDR(args_remaining);

    if isVectorList(x) == 0 {
        r_error("'object' must be a list or expression");
    }

    let fun = CAR(args_remaining);
    args_remaining = CDR(args_remaining);

    if isFunction(fun) == 0 {
        r_error("invalid 'f' argument");
    }

    let classes = CAR(args_remaining);
    args_remaining = CDR(args_remaining);

    if isString(classes) == 0 {
        r_error("invalid 'classes' argument");
    }

    let deflt = CAR(args_remaining);
    args_remaining = CDR(args_remaining);

    let how = CAR(args_remaining);

    if isString(how) == 0 {
        r_error("invalid 'how' argument");
    }

    let replace = LENGTH(how) > 0
        && char_cstr_bytes_opt(STRING_ELT(how, 0)).is_some_and(|name| name == b"replace");

    let n: R_xlen_t = XLENGTH(x);
    let ans = if replace {
        shallow_duplicate(x)
    } else {
        let a = Rf_allocVector(VECSXP_VAL, n as c_int);
        let names = getAttrib(x, R_NamesSymbol());
        if Rf_isNull(names) == 0 {
            setAttrib(a, R_NamesSymbol(), names);
        }
        a
    };
    let _ans_guard = protect(ans);

    for i in 0..n {
        let elt = VECTOR_ELT(x, i);
        let result = do_one(elt, fun, classes, deflt, replace, rho);
        SET_VECTOR_ELT(ans, i, result);
    }

    ans
}

// ---------------------------------------------------------------------------
// islistfactor_recursive -- recursively check if tree has only factor leaves
//
// Returns TRUE(1), FALSE(0) or NA_LOGICAL.
// ---------------------------------------------------------------------------

pub(crate) fn islistfactor_recursive(x: SEXP) -> c_int {
    if x.is_null() || x == R_NilValue() {
        return NA_LOGICAL;
    }
    let t = TYPEOF(x);

    match t {
        t if t == VECSXP_VAL || t == EXPRSXP_VAL => {
            let n = LENGTH(x);
            let mut ans = NA_LOGICAL;
            for i in 0..n {
                let is_lf = islistfactor_recursive(VECTOR_ELT(x, i as R_xlen_t));
                if is_lf == 0 {
                    return 0; // FALSE
                } else if is_lf == 1 {
                    ans = 1; // TRUE
                }
                // else isLF is NA -- keep going
            }
            ans
        }
        _ => {
            // Leaf: check if it's a factor
            isFactor(x)
        }
    }
}

// ---------------------------------------------------------------------------
// do_islistfactor -- is this a tree with only factor leaves?
//
// Checks whether X is a list (or expression) tree whose every leaf is a
// factor. If `recursive` is FALSE, only the top-level elements are checked.
// ---------------------------------------------------------------------------

pub fn do_islistfactor(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    checkArity(op, args);

    let x = CAR(args);
    if args.is_null()
        || args == R_NilValue()
        || CADR(args).is_null()
        || CADR(args) == R_NilValue()
    {
        return Rf_ScalarLogical(0); // FALSE for missing/bad args
    }
    let recursive = asBool2(CADR(args), _call);
    let n = length(x);

    if n == 0 || isVectorList(x) == 0 {
        return Rf_ScalarLogical(0); // FALSE
    }

    if !recursive {
        // Non-recursive: just check top-level elements
        for i in 0..n {
            if isFactor(VECTOR_ELT(x, i as R_xlen_t)) == 0 {
                return Rf_ScalarLogical(0); // FALSE
            }
        }
        return Rf_ScalarLogical(1); // TRUE
    } else {
        // Recursive: walk the entire tree
        let result = islistfactor_recursive(x);
        Rf_ScalarLogical(if result == 1 { 1 } else { 0 })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::constructors::*;

    use super::*;

    #[test]
    fn test_checkArgIsSymbol_null() {
        let _session = crate::sexp::session::RSession::new();
        // checkArgIsSymbol panics on null, so we don't test that directly
        // Test with a non-null value
        // Create a symbol
        let s = crate::sexp::symbol::install_str("x");
        let result = checkArgIsSymbol(s);
        assert_eq!(result, s);
    }

    #[test]
    fn test_islistfactor_recursive_null() {
        let _session = crate::sexp::session::RSession::new();
        let result = islistfactor_recursive(ptr::null_mut());
        assert_eq!(result, NA_LOGICAL);
    }

    #[test]
    fn test_islistfactor_recursive_nil() {
        let _session = crate::sexp::session::RSession::new();
        let result = islistfactor_recursive(R_NilValue());
        assert_eq!(result, NA_LOGICAL);
    }

    #[test]
    fn test_do_one_null() {
        let _session = crate::sexp::session::RSession::new();
        let result = do_one(
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            false,
            ptr::null_mut(),
        );
        // null is treated as empty list, returns empty VECSXP
        assert!(!result.is_null());
        assert_eq!(TYPEOF(result), VECSXP_VAL);
    }

    #[test]
    fn test_do_one_nil() {
        let _session = crate::sexp::session::RSession::new();
        let result = do_one(
            R_NilValue(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            false,
            ptr::null_mut(),
        );
        assert!(!result.is_null());
        assert_eq!(TYPEOF(result), VECSXP_VAL);
    }

    #[test]
    fn test_asLogical_null() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(asLogical(ptr::null_mut()), NA_LOGICAL);
        assert_eq!(asLogical(R_NilValue()), NA_LOGICAL);
    }

    #[test]
    fn test_isVectorList_null() {
        let _session = crate::sexp::session::RSession::new();
        {
            assert_eq!(isVectorList(ptr::null_mut()), 0);
            assert_eq!(isVectorList(R_NilValue()), 0);
        }
    }

    #[test]
    fn test_isFunction_null() {
        let _session = crate::sexp::session::RSession::new();
        {
            assert_eq!(isFunction(ptr::null_mut()), 0);
            assert_eq!(isFunction(R_NilValue()), 0);
        }
    }

    #[test]
    fn test_isString_null() {
        let _session = crate::sexp::session::RSession::new();
        {
            assert_eq!(isString(ptr::null_mut()), 0);
            assert_eq!(isString(R_NilValue()), 0);
        }
    }

    #[test]
    fn test_isVector_null() {
        let _session = crate::sexp::session::RSession::new();
        {
            assert_eq!(isVector(ptr::null_mut()), 0);
            assert_eq!(isVector(R_NilValue()), 0);
        }
    }

    #[test]
    fn test_length_null() {
        let _session = crate::sexp::session::RSession::new();
        {
            assert_eq!(length(ptr::null_mut()), 0);
            assert_eq!(length(R_NilValue()), 0);
        }
    }

    #[test]
    fn test_Seql_same_pointer() {
        let _session = crate::sexp::session::RSession::new();
        let s = crate::sexp::symbol::install_str("test");
        assert_eq!(Seql(s, s), 1);
    }

    #[test]
    fn test_Seql_null() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(Seql(ptr::null_mut(), ptr::null_mut()), 1);
        assert_eq!(Seql(ptr::null_mut(), R_NilValue()), 0);
    }

    #[test]
    fn test_isFactor_null() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(isFactor(ptr::null_mut()), 0);
        assert_eq!(isFactor(R_NilValue()), 0);
    }

    #[test]
    fn test_do_islistfactor_null() {
        let _session = crate::sexp::session::RSession::new();
        let result = do_islistfactor(
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        assert!(!result.is_null());
        assert_eq!(TYPEOF(result), LGLSXP_VAL);
        assert_eq!(LOGICAL_ELT(result, 0), 0); // FALSE for non-list
    }

    #[test]
    fn test_do_islistfactor_empty_list() {
        let _session = crate::sexp::session::RSession::new();
        let empty = Rf_allocVector(VECSXP_VAL, 0);
        let _empty_guard = protect(empty);
        let result = do_islistfactor(
            ptr::null_mut(),
            ptr::null_mut(),
            // args: list(x, recursive) as pairlist
            Rf_cons(empty, Rf_cons(Rf_ScalarLogical(1), R_NilValue())),
            ptr::null_mut(),
        );
        assert!(!result.is_null());
        assert_eq!(LOGICAL_ELT(result, 0), 0); // FALSE for empty list
    }

    #[test]
    fn test_MARK_NOT_MUTABLE_null() {
        let _session = crate::sexp::session::RSession::new();
        MARK_NOT_MUTABLE(ptr::null_mut()); // Should not crash
    }

    #[test]
    fn test_INCREMENT_NAMED_null() {
        let _session = crate::sexp::session::RSession::new();
        INCREMENT_NAMED(ptr::null_mut()); // Should not crash
    }

    #[test]
    fn test_MAYBE_REFERENCED_null() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(MAYBE_REFERENCED(ptr::null_mut()), false);
    }

    #[test]
    fn test_MAYBE_SHARED_null() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(MAYBE_SHARED(ptr::null_mut()), false);
    }

    #[test]
    fn test_LCONS() {
        let _session = crate::sexp::session::RSession::new();
        let car = Rf_ScalarInteger(1);
        let cdr = Rf_ScalarInteger(2);
        let cell = LCONS(car, cdr);
        assert!(!cell.is_null());
        assert_eq!(TYPEOF(cell), SEXPTYPE::LANGSXP);
        assert_eq!(CAR(cell), car);
        assert_eq!(CDR(cell), cdr);
    }

    #[test]
    fn test_R_typeToChar_local() {
        let _session = crate::sexp::session::RSession::new();
        let iv = Rf_allocVector(INTSXP_VAL, 1);
        assert_eq!(R_typeToChar_local(iv), "integer");
    }
}
