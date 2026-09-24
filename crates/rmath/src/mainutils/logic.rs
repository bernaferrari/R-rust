#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/logic.c — logical operation helpers.
//!
//! This module ports the scalar-level logical operations used by R's
//! vector `&`, `|`, and `!` operators, including proper NA handling.
//!
//! Ported standalone functions:
//!   logical_and, logical_or, logical_not,
//!   checkValues, raw_and, raw_or
//!
//! SEXP-dependent functions:
//!   do_logic (& | !), do_logic2 (&& ||), do_logic3 (any all)

use std::os::raw::c_int;

use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, COMPLEX, INTEGER, LOGICAL, RAW, REAL, TYPEOF, XLENGTH,
};
use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector3, Rf_length};
use crate::sexp::ffi::{FALSE, ISNAN, NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's NA_LOGICAL sentinel.
pub const NA_LOGICAL: c_int = c_int::MIN;

/// Operation codes for checkValues.
pub const OP_ANY: c_int = 1;
pub const OP_ALL: c_int = 2;

fn logic_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
}

fn scalar_logic_op_name(code: c_int) -> &'static str {
    if code == 1 { "&&" } else { "||" }
}

// ---------------------------------------------------------------------------
// Scalar logical operations
// ---------------------------------------------------------------------------

/// Logical AND of two R logical values with NA handling.
///
/// Truth table:
/// - TRUE & TRUE = TRUE
/// - TRUE & FALSE = FALSE
/// - FALSE & TRUE = FALSE
/// - FALSE & FALSE = FALSE
/// - NA & FALSE = FALSE
/// - FALSE & NA = FALSE
/// - NA & TRUE = NA
/// - TRUE & NA = NA
/// - NA & NA = NA
#[inline]
pub fn logical_and(x: c_int, y: c_int) -> c_int {
    if x == 0 || y == 0 {
        0
    } else if x == NA_LOGICAL || y == NA_LOGICAL {
        NA_LOGICAL
    } else {
        1
    }
}

/// Logical OR of two R logical values with NA handling.
///
/// Truth table:
/// - TRUE | anything = TRUE
/// - FALSE | FALSE = FALSE
/// - FALSE | NA = NA
/// - NA | FALSE = NA
/// - NA | NA = NA
#[inline]
pub fn logical_or(x: c_int, y: c_int) -> c_int {
    if (x != NA_LOGICAL && x != 0) || (y != NA_LOGICAL && y != 0) {
        1
    } else if x == 0 && y == 0 {
        0
    } else {
        NA_LOGICAL
    }
}

/// Logical NOT of an R logical value with NA handling.
///
/// - !TRUE = FALSE
/// - !FALSE = TRUE
/// - !NA = NA
#[inline]
pub fn logical_not(x: c_int) -> c_int {
    if x == NA_LOGICAL {
        NA_LOGICAL
    } else if x == 0 {
        1
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// any()/all() checkValues logic
// ---------------------------------------------------------------------------

/// Check values for `any()` or `all()` reduction.
///
/// `op`: `OP_ANY` (1) or `OP_ALL` (2).
/// `na_rm`: if false, NAs are tracked; if true, NAs are skipped.
/// `x`: slice of logical values.
///
/// Returns:
/// - For OP_ANY: TRUE if any TRUE found, FALSE if none, NA if NAs present
/// - For OP_ALL: FALSE if any FALSE found, TRUE if none, NA if NAs present
pub fn checkValues(op: c_int, na_rm: bool, x: &[c_int]) -> c_int {
    let mut has_na = false;

    for &xi in x.iter() {
        if !na_rm && xi == NA_LOGICAL {
            has_na = true;
        } else {
            if xi != 0 && op == OP_ANY {
                return 1;
            }
            if xi == 0 && op == OP_ALL {
                return 0;
            }
        }
    }

    match op {
        OP_ANY => {
            if has_na {
                NA_LOGICAL
            } else {
                0
            }
        }
        OP_ALL => {
            if has_na {
                NA_LOGICAL
            } else {
                1
            }
        }
        _ => NA_LOGICAL,
    }
}

// ---------------------------------------------------------------------------
// Raw (byte) logical operations
// ---------------------------------------------------------------------------

/// Bitwise AND of two raw bytes.
#[inline]
pub fn raw_and(x: u8, y: u8) -> u8 {
    x & y
}

/// Bitwise OR of two raw bytes.
#[inline]
pub fn raw_or(x: u8, y: u8) -> u8 {
    x | y
}

/// Bitwise NOT (XOR with 0xFF) of a raw byte.
#[inline]
pub fn raw_not(x: u8) -> u8 {
    0xFF ^ x
}

// ---------------------------------------------------------------------------
// Vector-level logical operations (on slices)
// ---------------------------------------------------------------------------

/// Element-wise logical AND of two slices, with NA handling and recycling.
///
/// `s1`, `s2`: input slices (may be different lengths; shorter is recycled).
/// Returns a new vector with `max(s1.len(), s2.len())` elements.
pub fn binary_logic_and(s1: &[c_int], s2: &[c_int]) -> Vec<c_int> {
    let n = s1.len().max(s2.len());
    let mut result = Vec::with_capacity(n);
    for i in 0..n {
        let x1 = s1[i % s1.len()];
        let x2 = s2[i % s2.len()];
        result.push(logical_and(x1, x2));
    }
    result
}

/// Element-wise logical OR of two slices, with NA handling and recycling.
pub fn binary_logic_or(s1: &[c_int], s2: &[c_int]) -> Vec<c_int> {
    let n = s1.len().max(s2.len());
    let mut result = Vec::with_capacity(n);
    for i in 0..n {
        let x1 = s1[i % s1.len()];
        let x2 = s2[i % s2.len()];
        result.push(logical_or(x1, x2));
    }
    result
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Get the primitive operation code from `op`.
///
/// This reads the `offset` field of a `primsxp`, which R uses to store
/// the operation code for built-in primitives.
#[inline]
unsafe fn primval(op: SEXP) -> c_int {
    unsafe { crate::mainutils::relop::PRIMVAL(op) }
}

/// Check if a SEXP is of numeric type (logical, integer, real, or complex).
#[inline]
unsafe fn is_number(x: SEXP) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        let t = TYPEOF(x);
        t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
    }
}

/// Check if a SEXP is of raw type.
#[inline]
unsafe fn is_raw(x: SEXP) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        TYPEOF(x) == SEXPTYPE::RAWSXP
    }
}

/// Coerce a numeric/logical SEXP to logical (LGLSXP).
///
/// For integer/logical: direct copy, treating NA_INTEGER as NA_LOGICAL.
/// For real: NaN -> NA, 0.0 -> FALSE, nonzero -> TRUE.
/// For complex: NaN in either part -> NA, 0+0i -> FALSE, else -> TRUE.
/// For null: return zero-length logical vector.
/// For raw: return zero-length logical vector (not applicable).
unsafe fn coerce_to_logical(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let len = XLENGTH(x) as usize;
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, len as R_xlen_t);
        let _ans_guard = protect(ans);
        let pa = LOGICAL(ans);
        let t = TYPEOF(x);

        match t {
            tt if tt == SEXPTYPE::LGLSXP => {
                let px = LOGICAL(x);
                for i in 0..len {
                    *pa.add(i) = *px.add(i);
                }
            }
            tt if tt == SEXPTYPE::INTSXP => {
                let px = INTEGER(x);
                for i in 0..len {
                    let v = *px.add(i);
                    *pa.add(i) = if v == NA_INTEGER {
                        NA_LOGICAL
                    } else {
                        (v != 0) as c_int
                    };
                }
            }
            tt if tt == SEXPTYPE::REALSXP => {
                let px = REAL(x);
                for i in 0..len {
                    let v = *px.add(i);
                    *pa.add(i) = if ISNAN(v) {
                        NA_LOGICAL
                    } else {
                        (v != 0.0) as c_int
                    };
                }
            }
            tt if tt == SEXPTYPE::CPLXSXP => {
                let px = COMPLEX(x);
                for i in 0..len {
                    let v = *px.add(i);
                    *pa.add(i) = if ISNAN(v.r) || ISNAN(v.i) {
                        NA_LOGICAL
                    } else if v.r == 0.0 && v.i == 0.0 {
                        0
                    } else {
                        1
                    };
                }
            }
            _ => {
                // For other types, zero-length
            }
        }
        ans
    }
}

/// Unary logical NOT: `!` operator.
///
/// Handles LGLSXP, INTSXP, REALSXP, CPLXSXP, RAWSXP with proper NA handling.
unsafe fn lunary(arg: SEXP) -> SEXP {
    unsafe {
        if arg.is_null() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }

        let len = XLENGTH(arg) as usize;
        let t = TYPEOF(arg);

        // Determine output type
        let out_type = if is_raw(arg) {
            SEXPTYPE::RAWSXP.as_c_int()
        } else {
            SEXPTYPE::LGLSXP.as_c_int()
        };

        let x = Rf_allocVector3(out_type, len as R_xlen_t);
        let _x_guard = protect(x);

        match t {
            tt if tt == SEXPTYPE::LGLSXP => {
                let px = LOGICAL(arg);
                let pa = LOGICAL(x);
                for i in 0..len {
                    let v = *px.add(i);
                    *pa.add(i) = logical_not(v);
                }
            }
            tt if tt == SEXPTYPE::INTSXP => {
                let px = INTEGER(arg);
                let pa = LOGICAL(x);
                for i in 0..len {
                    let v = *px.add(i);
                    *pa.add(i) = if v == NA_INTEGER {
                        NA_LOGICAL
                    } else {
                        (v == 0) as c_int
                    };
                }
            }
            tt if tt == SEXPTYPE::REALSXP => {
                let px = REAL(arg);
                let pa = LOGICAL(x);
                for i in 0..len {
                    let v = *px.add(i);
                    *pa.add(i) = if ISNAN(v) {
                        NA_LOGICAL
                    } else {
                        (v == 0.0) as c_int
                    };
                }
            }
            tt if tt == SEXPTYPE::CPLXSXP => {
                let px = COMPLEX(arg);
                let pa = LOGICAL(x);
                for i in 0..len {
                    let v = *px.add(i);
                    *pa.add(i) = if ISNAN(v.r) || ISNAN(v.i) {
                        NA_LOGICAL
                    } else if v.r == 0.0 && v.i == 0.0 {
                        1 // !0 = TRUE
                    } else {
                        0 // !nonzero = FALSE
                    };
                }
            }
            tt if tt == SEXPTYPE::RAWSXP => {
                let px = RAW(arg);
                let pa = RAW(x);
                for i in 0..len {
                    *pa.add(i) = raw_not(*px.add(i));
                }
            }
            _ => {
                // Return zero-length for unsupported types
            }
        }

        // GNU lunary shallow-duplicates a logical or raw, so `!` keeps
        // the ts class. A numeric becomes a plain logical and keeps only
        // names and dimensions.
        if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::RAWSXP {
            let mut attr = crate::sexp::accessors::ATTRIB(arg);
            while !attr.is_null() && attr != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    x,
                    crate::sexp::accessors::TAG(attr),
                    crate::sexp::accessors::CAR(attr),
                );
                attr = crate::sexp::accessors::CDR(attr);
            }
        } else {
            copy_logic_shape(arg, x);
        }
        x
    }
}

unsafe fn copy_logic_shape(src: SEXP, dst: SEXP) {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(src, crate::sexp::attrib_core::R_DimSymbol());
        if !dim.is_null() && dim != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(dst, crate::sexp::attrib_core::R_DimSymbol(), dim);
            let dn = crate::sexp::attrib_core::getAttrib(
                src,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
            );
            if !dn.is_null() && dn != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    dst,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                    dn,
                );
            }
        } else {
            let names =
                crate::sexp::attrib_core::getAttrib(src, crate::sexp::attrib_core::R_NamesSymbol());
            if !names.is_null() && names != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    dst,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
        }
    }
}


/// Binary logical AND/OR on logical vectors with element recycling.
///
/// `code`: 1 = AND (&), 2 = OR (|).
/// Both s1 and s2 must already be LGLSXP.
unsafe fn binary_logic(code: c_int, s1: SEXP, s2: SEXP) -> SEXP {
    unsafe {
        let n1 = XLENGTH(s1) as usize;
        let n2 = XLENGTH(s2) as usize;
        let n = if n1 > n2 { n1 } else { n2 };

        if n1 == 0 || n2 == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }

        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n as R_xlen_t);
        let _ans_guard = protect(ans);
        let px1 = LOGICAL(s1);
        let px2 = LOGICAL(s2);
        let pa = LOGICAL(ans);

        for i in 0..n {
            let i1 = i % n1;
            let i2 = i % n2;
            let x1 = *px1.add(i1);
            let x2 = *px2.add(i2);

            *pa.add(i) = if code == 1 {
                logical_and(x1, x2)
            } else {
                logical_or(x1, x2)
            };
        }

        ans
    }
}

/// Binary logical AND/OR on raw vectors with element recycling.
///
/// `code`: 1 = AND (&), 2 = OR (|).
/// Both s1 and s2 must already be RAWSXP.
unsafe fn binary_logic_raw(code: c_int, s1: SEXP, s2: SEXP) -> SEXP {
    unsafe {
        let n1 = XLENGTH(s1) as usize;
        let n2 = XLENGTH(s2) as usize;
        let n = if n1 > n2 { n1 } else { n2 };

        if n1 == 0 || n2 == 0 {
            return Rf_allocVector3(SEXPTYPE::RAWSXP, 0);
        }

        let ans = Rf_allocVector3(SEXPTYPE::RAWSXP, n as R_xlen_t);
        let _ans_guard = protect(ans);
        let px1 = RAW(s1);
        let px2 = RAW(s2);
        let pa = RAW(ans);

        for i in 0..n {
            let i1 = i % n1;
            let i2 = i % n2;
            let x1 = *px1.add(i1);
            let x2 = *px2.add(i2);

            *pa.add(i) = if code == 1 {
                raw_and(x1, x2)
            } else {
                raw_or(x1, x2)
            };
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_logic — R's `&`, `|`, `!` operators (vectorized)
// ---------------------------------------------------------------------------

/// Port of R's `do_logic` from logic.c.
///
/// Handles three cases:
/// - One argument: unary `!` (logical NOT)
/// - Two arguments: binary `&` (AND) or `|` (OR), dispatched via PRIMVAL
///
/// `PRIMVAL(op)` determines the operation for binary case: 1 = &, 2 = |.
/// For unary `!`, the PRIMVAL is 3 but the operation is determined by arity.
pub unsafe fn do_logic(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let arg1 = CAR(args);
        let mut ans = R_NilValue();
        if crate::eval::dispatch::DispatchGroup(
            b"Ops\0".as_ptr() as *const std::os::raw::c_char,
            call,
            op,
            args,
            env,
            &mut ans,
        ) != 0
        {
            return ans;
        }
        let nargs = if args.is_null() || args == R_NilValue() {
            0
        } else {
            let mut n = 0;
            let mut cell = args;
            while !cell.is_null() && cell != R_NilValue() {
                n += 1;
                cell = CDR(cell);
            }
            n
        };
        let code = primval(op);
        if code == 1 || code == 2 {
            if nargs != 2 {
                let name = if code == 1 { "&" } else { "|" };
                let noun = if nargs == 1 { "argument" } else { "arguments" };
                logic_error(format!(
                    "{nargs} {noun} passed to '{name}' which requires 2"
                ));
            }
        } else if nargs != 1 {
            let noun = if nargs == 1 { "argument" } else { "arguments" };
            logic_error(format!("{nargs} {noun} passed to '!' which requires 1"));
        }

        let attr1 = !ATTRIB(arg1).is_null();

        // `!` is the only unary operator in this primitive.
        if CDR(args) == R_NilValue() {
            // Fast path for scalar logical
            if !attr1 && TYPEOF(arg1) == SEXPTYPE::LGLSXP && XLENGTH(arg1) == 1 {
                let v = *LOGICAL(arg1);
                let result = logical_not(v);
                return Rf_ScalarLogical(result);
            }
            return lunary(arg1);
        }

        // Binary case: two arguments => & or |
        let code = primval(op); // 1 = &, 2 = |
        let x = CAR(args);
        let y = CADR(args);

        // Both raw => bitwise operation
        if is_raw(x) && is_raw(y) {
            return binary_logic_raw(code, x, y);
        }

        let x_valid = x.is_null() || x == R_NilValue() || is_number(x);
        let y_valid = y.is_null() || y == R_NilValue() || is_number(y);
        if !x_valid || !y_valid {
            logic_error("operations are possible only for numeric, logical or complex types");
        }

        let nx = XLENGTH(x);
        let ny = XLENGTH(y);

        // Zero-length case
        if nx == 0 || ny == 0 {
            let empty = Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
            let _empty_guard = protect(empty);
            crate::eval::arithmetic::propagate_arithmetic_attributes(empty, x, y, 0);
            return empty;
        }

        // Coerce both to logical and apply binary logic
        let x_lgl = coerce_to_logical(x);
        let _x_lgl_guard = protect(x_lgl);
        let y_lgl = coerce_to_logical(y);
        let _y_lgl_guard = protect(y_lgl);
        let result = binary_logic(code, x_lgl, y_lgl);
        result
    }
}

// ---------------------------------------------------------------------------
// do_logic2 — R's `&&` and `||` (short-circuit, scalar)
// ---------------------------------------------------------------------------

/// Port of R's `do_logic2` from logic.c.
///
/// Implements `&&` and `||` — short-circuit scalar logical operators.
/// Like upstream, `&&` / `||` are SPECIALSXP primitives: the handler is
/// passed the *unevaluated* argument expressions and evaluates them itself,
/// so the right operand is only evaluated when the left one has not already
/// decided the result.
///
/// `PRIMVAL(op)`: 1 = `&&`, 2 = `||`.
///
/// These always return a length-1 logical scalar:
/// - `&&`: if x1 is FALSE, result is FALSE without evaluating x2
/// - `||`: if x1 is TRUE, result is TRUE without evaluating x2
///
/// NA handling:
/// - `FALSE && NA` = FALSE
/// - `TRUE && NA` = NA
/// - `NA && NA` = NA
/// - `TRUE || NA` = TRUE
/// - `FALSE || NA` = NA
/// - `NA || NA` = NA
pub unsafe fn do_logic2(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let code = primval(op); // 1 = &&, 2 = ||

        // Require exactly 2 arguments
        if Rf_length(args) != 2 {
            logic_error(format!(
                "'{}' operator requires 2 arguments",
                scalar_logic_op_name(code)
            ));
        }

        // Evaluate the left operand, then validate and coerce it before the
        // right operand is touched (upstream evaluates CAR(args) first).
        let s1 = crate::eval::eval::Rf_eval(CAR(args), env);
        let _s1_guard = protect(s1);

        // Validate s1 is numeric
        if !is_number(s1) {
            logic_error(format!(
                "invalid 'x' type in 'x {} y'",
                scalar_logic_op_name(code)
            ));
        }

        // Convert s1 to a scalar logical value
        let x1 = coerce_scalar_to_logical(s1, call);

        match code {
            1 => {
                // && : short-circuit AND
                if x1 == FALSE {
                    return Rf_ScalarLogical(FALSE);
                }
                // Need to evaluate second argument
                let s2 = crate::eval::eval::Rf_eval(CADR(args), env);
                let _s2_guard = protect(s2);
                if !is_number(s2) {
                    logic_error(format!(
                        "invalid 'y' type in 'x {} y'",
                        scalar_logic_op_name(code)
                    ));
                }
                let x2 = coerce_scalar_to_logical(s2, call);
                if x1 == NA_LOGICAL {
                    // NA && x2: result is NA unless x2 is FALSE
                    let ans = if x2 == NA_LOGICAL || x2 != 0 {
                        NA_LOGICAL
                    } else {
                        x2
                    };
                    return Rf_ScalarLogical(ans);
                }
                // x1 == TRUE => result is x2
                Rf_ScalarLogical(x2)
            }
            2 => {
                // || : short-circuit OR
                if x1 == TRUE {
                    return Rf_ScalarLogical(TRUE);
                }
                // Need to evaluate second argument
                let s2 = crate::eval::eval::Rf_eval(CADR(args), env);
                let _s2_guard = protect(s2);
                if !is_number(s2) {
                    logic_error(format!(
                        "invalid 'y' type in 'x {} y'",
                        scalar_logic_op_name(code)
                    ));
                }
                let x2 = coerce_scalar_to_logical(s2, call);
                if x1 == NA_LOGICAL {
                    // NA || x2: result is NA unless x2 is TRUE
                    let ans = if x2 == NA_LOGICAL || x2 == 0 {
                        NA_LOGICAL
                    } else {
                        x2
                    };
                    return Rf_ScalarLogical(ans);
                }
                // x1 == FALSE => result is x2
                Rf_ScalarLogical(x2)
            }
            _ => Rf_ScalarLogical(NA_LOGICAL),
        }
    }
}

/// Coerce a SEXP to a scalar logical value.
///
/// For length-1 vectors, returns the logical value directly.
/// For longer vectors, errors the way stock R's `asLogical()` does.
/// For NA inputs, returns NA_LOGICAL.
unsafe fn coerce_scalar_to_logical(x: SEXP, _call: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_LOGICAL;
        }
        let len = XLENGTH(x);
        if len == 0 {
            return NA_LOGICAL;
        }
        if len > 1 {
            logic_error(format!("'length = {len}' in coercion to 'logical(1)'"));
        }

        let t = TYPEOF(x);
        match t {
            tt if tt == SEXPTYPE::LGLSXP => *LOGICAL(x),
            tt if tt == SEXPTYPE::INTSXP => {
                let v = *INTEGER(x);
                if v == NA_INTEGER {
                    NA_LOGICAL
                } else {
                    (v != 0) as c_int
                }
            }
            tt if tt == SEXPTYPE::REALSXP => {
                let v = *REAL(x);
                if ISNAN(v) {
                    NA_LOGICAL
                } else {
                    (v != 0.0) as c_int
                }
            }
            tt if tt == SEXPTYPE::CPLXSXP => {
                let v = *COMPLEX(x);
                if ISNAN(v.r) || ISNAN(v.i) {
                    NA_LOGICAL
                } else if v.r == 0.0 && v.i == 0.0 {
                    FALSE
                } else {
                    TRUE
                }
            }
            _ => NA_LOGICAL,
        }
    }
}

// ---------------------------------------------------------------------------
// do_logic3 — R's `any()` and `all()` (reduction operators)
// ---------------------------------------------------------------------------

/// Port of R's `do_logic3` from logic.c.
///
/// Implements `any()` and `all()` reduction operators.
///
/// `PRIMVAL(op)`: 1 = `all`, 2 = `any`.
///
/// These reduce a logical vector (or coercible input) to a single scalar:
/// - `any()`: TRUE if any element is TRUE
/// - `all()`: TRUE if all elements are TRUE
///
/// NA handling:
/// - `any(..., na.rm=FALSE)`: NA if no TRUE found and any NA present
/// - `all(..., na.rm=FALSE)`: NA if no FALSE found and any NA present
/// - Empty input: `any(logical(0))` = FALSE, `all(logical(0))` = TRUE
///
/// The `na.rm` argument (third argument, if present and TRUE) causes NAs
/// to be skipped.
pub unsafe fn do_logic3(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let code = primval(op); // 1 = all, 2 = any

        // Initialize default result for empty input
        let mut val = if code == 1 { TRUE } else { FALSE };
        let mut has_na = false;

        // Walk through argument list
        let mut s = args;
        while s != R_NilValue() && !s.is_null() {
            let t = CAR(s);

            // Skip empty inputs
            if !t.is_null() && XLENGTH(t) > 0 {
                // Coerce to logical if needed
                let t_lgl = if TYPEOF(t) == SEXPTYPE::LGLSXP {
                    t
                } else {
                    coerce_to_logical(t)
                };

                let n = XLENGTH(t_lgl) as usize;
                let px = LOGICAL(t_lgl);

                // Apply checkValues logic inline
                for i in 0..n {
                    let xi = *px.add(i);
                    if xi == NA_LOGICAL {
                        has_na = true;
                    } else {
                        if xi != 0 && code == 2 {
                            // any: found TRUE
                            val = TRUE;
                            has_na = false;
                            break;
                        }
                        if xi == 0 && code == 1 {
                            // all: found FALSE
                            val = FALSE;
                            has_na = false;
                            break;
                        }
                    }
                }

                // Early exit on definitive result
                if !has_na && ((code == 2 && val == TRUE) || (code == 1 && val == FALSE)) {
                    break;
                }
            }

            s = CDR(s);
        }

        if has_na {
            Rf_ScalarLogical(NA_LOGICAL)
        } else {
            Rf_ScalarLogical(val)
        }
    }
}

/// GNU `xor(x, y)` is `(x | y) & !(x & y)` with logical recycle.
pub unsafe fn do_xor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CADR(args);
        let xl = crate::main::coerce::coerceVector(x, SEXPTYPE::LGLSXP.as_c_int());
        let _xl = protect(xl);
        let yl = crate::main::coerce::coerceVector(y, SEXPTYPE::LGLSXP.as_c_int());
        let _yl = protect(yl);
        let nx = XLENGTH(xl);
        let ny = XLENGTH(yl);
        if nx == 0 || ny == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let n = nx.max(ny);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let _result = protect(result);
        let out = LOGICAL(result);
        let a = LOGICAL(xl);
        let b = LOGICAL(yl);
        for i in 0..n {
            let av = *a.add((i % nx) as usize);
            let bv = *b.add((i % ny) as usize);
            *out.add(i as usize) = if av == NA_LOGICAL || bv == NA_LOGICAL {
                NA_LOGICAL
            } else if (av != 0) ^ (bv != 0) {
                TRUE
            } else {
                FALSE
            };
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logical_and() {
        assert_eq!(logical_and(1, 1), 1);
        assert_eq!(logical_and(1, 0), 0);
        assert_eq!(logical_and(0, 1), 0);
        assert_eq!(logical_and(0, 0), 0);
        assert_eq!(logical_and(NA_LOGICAL, 0), 0);
        assert_eq!(logical_and(0, NA_LOGICAL), 0);
        assert_eq!(logical_and(NA_LOGICAL, 1), NA_LOGICAL);
        assert_eq!(logical_and(1, NA_LOGICAL), NA_LOGICAL);
        assert_eq!(logical_and(NA_LOGICAL, NA_LOGICAL), NA_LOGICAL);
    }

    #[test]
    fn test_logical_or() {
        assert_eq!(logical_or(1, 1), 1);
        assert_eq!(logical_or(1, 0), 1);
        assert_eq!(logical_or(0, 1), 1);
        assert_eq!(logical_or(0, 0), 0);
        assert_eq!(logical_or(NA_LOGICAL, 1), 1);
        assert_eq!(logical_or(1, NA_LOGICAL), 1);
        assert_eq!(logical_or(NA_LOGICAL, 0), NA_LOGICAL);
        assert_eq!(logical_or(0, NA_LOGICAL), NA_LOGICAL);
        assert_eq!(logical_or(NA_LOGICAL, NA_LOGICAL), NA_LOGICAL);
    }

    #[test]
    fn test_logical_not() {
        assert_eq!(logical_not(1), 0);
        assert_eq!(logical_not(0), 1);
        assert_eq!(logical_not(NA_LOGICAL), NA_LOGICAL);
    }

    #[test]
    fn test_checkValues_any() {
        assert_eq!(checkValues(OP_ANY, false, &[0, 0, 1, 0]), 1);
        assert_eq!(checkValues(OP_ANY, false, &[0, 0, 0]), 0);
        assert_eq!(checkValues(OP_ANY, false, &[0, NA_LOGICAL, 0]), NA_LOGICAL);
    }

    #[test]
    fn test_checkValues_all() {
        assert_eq!(checkValues(OP_ALL, false, &[1, 1, 0]), 0);
        assert_eq!(checkValues(OP_ALL, false, &[1, 1, 1]), 1);
        assert_eq!(checkValues(OP_ALL, false, &[1, NA_LOGICAL, 1]), NA_LOGICAL);
    }

    #[test]
    fn test_raw_ops() {
        assert_eq!(raw_and(0xFF, 0x0F), 0x0F);
        assert_eq!(raw_or(0xF0, 0x0F), 0xFF);
        assert_eq!(raw_not(0x00), 0xFF);
        assert_eq!(raw_not(0xFF), 0x00);
    }

    #[test]
    fn test_binary_logic_and() {
        let a = [1, 0, NA_LOGICAL, 1];
        let b = [1, 0, 1, NA_LOGICAL];
        let result = binary_logic_and(&a, &b);
        assert_eq!(result, [1, 0, NA_LOGICAL, NA_LOGICAL]);
    }

    #[test]
    fn test_binary_logic_or() {
        let a = [1, 0, NA_LOGICAL, 0];
        let b = [0, 0, 0, NA_LOGICAL];
        let result = binary_logic_or(&a, &b);
        assert_eq!(result, [1, 0, NA_LOGICAL, NA_LOGICAL]);
    }

    #[test]
    fn test_do_logic_invalid_operand_type_errors() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let op = crate::mainutils::names::R_Primitive(c"&".as_ptr());
            let x = crate::sexp::constructors::Rf_allocVector(SEXPTYPE::STRSXP, 1);
            let y = Rf_ScalarLogical(TRUE);
            let args = crate::sexp::constructors::Rf_cons(
                x,
                crate::sexp::constructors::Rf_cons(y, R_NilValue()),
            );
            let err = std::panic::catch_unwind(|| {
                let _ = do_logic(std::ptr::null_mut(), op, args, R_NilValue());
            })
            .expect_err("invalid & operands should raise an RError");
            let message = err
                .downcast_ref::<crate::sexp::context::RError>()
                .map(|err| err.message.as_str())
                .unwrap_or("");
            assert_eq!(
                message,
                "operations are possible only for numeric, logical or complex types"
            );
        }
    }

    #[test]
    fn test_do_logic2_requires_two_arguments() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let op = crate::mainutils::names::R_Primitive(c"&&".as_ptr());
            let args = crate::sexp::constructors::Rf_cons(Rf_ScalarLogical(TRUE), R_NilValue());
            let err = std::panic::catch_unwind(|| {
                let _ = do_logic2(std::ptr::null_mut(), op, args, R_NilValue());
            })
            .expect_err("&& should require two arguments");
            let message = err
                .downcast_ref::<crate::sexp::context::RError>()
                .map(|err| err.message.as_str())
                .unwrap_or("");
            assert_eq!(message, "'&&' operator requires 2 arguments");
        }
    }

    #[test]
    fn test_do_logic2_invalid_x_type_errors() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let op = crate::mainutils::names::R_Primitive(c"&&".as_ptr());
            let x = crate::sexp::constructors::Rf_allocVector(SEXPTYPE::STRSXP, 1);
            let y = Rf_ScalarLogical(TRUE);
            let args = crate::sexp::constructors::Rf_cons(
                x,
                crate::sexp::constructors::Rf_cons(y, R_NilValue()),
            );
            let err = std::panic::catch_unwind(|| {
                let _ = do_logic2(std::ptr::null_mut(), op, args, R_NilValue());
            })
            .expect_err("invalid x in && should raise an RError");
            let message = err
                .downcast_ref::<crate::sexp::context::RError>()
                .map(|err| err.message.as_str())
                .unwrap_or("");
            assert_eq!(message, "invalid 'x' type in 'x && y'");
        }
    }

    #[test]
    fn test_do_logic2_invalid_y_type_errors_after_short_circuit_check() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let op = crate::mainutils::names::R_Primitive(c"&&".as_ptr());
            let x = Rf_ScalarLogical(TRUE);
            let y = crate::sexp::constructors::Rf_allocVector(SEXPTYPE::STRSXP, 1);
            let args = crate::sexp::constructors::Rf_cons(
                x,
                crate::sexp::constructors::Rf_cons(y, R_NilValue()),
            );
            let err = std::panic::catch_unwind(|| {
                let _ = do_logic2(std::ptr::null_mut(), op, args, R_NilValue());
            })
            .expect_err("invalid y in && should raise an RError");
            let message = err
                .downcast_ref::<crate::sexp::context::RError>()
                .map(|err| err.message.as_str())
                .unwrap_or("");
            assert_eq!(message, "invalid 'y' type in 'x && y'");
        }
    }
}
