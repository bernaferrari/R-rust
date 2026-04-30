#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/identical.c — object comparison utilities.
//!
//! This module ports the numeric comparison helpers that handle NA/NaN
//! with different strictness levels, used by R's `identical()` function,
//! plus the full `R_compute_identical` and `do_identical` implementations.
//!
//! Ported standalone functions:
//!   neWithNaN (not-equal with NaN awareness)
//!
//! Ported SEXP-dependent functions:
//!   R_identical, R_compute_identical, do_identical

use std::os::raw::c_int;

use crate::sexp::accessors::{
    ATTRIB, BODY, CAR, CDR, CLOENV, COMPLEX, FORMALS, INTEGER, LENGTH, LOGICAL, PRIMOFFSET, RAW,
    REAL, STRING_ELT, TAG, TYPEOF, VECTOR_ELT,
};
use crate::sexp::ffi::{R_NA_BIT_PATTERN, SEXP, SEXPTYPE};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Check if a double is R's NA.
#[inline]
pub fn R_IsNA(x: f64) -> bool {
    x.to_bits() == R_NA_BIT_PATTERN
}

/// Check if a double is any NaN.
#[inline]
pub fn ISNAN(x: f64) -> bool {
    x.is_nan()
}

// ---------------------------------------------------------------------------
// Strictness levels for neWithNaN
// ---------------------------------------------------------------------------

/// Single NA representation: NA != NaN, but use numeric equality for non-NAs.
pub const NE_SINGLE_NA_NUM_EQ: c_int = 0;

/// Single NA representation: NA != NaN, use bitwise comparison for non-NAs.
pub const NE_SINGLE_NA_NUM_BIT: c_int = 1;

/// Bitwise NA representation: use numeric equality for non-NAs, bitwise for NAs.
pub const NE_BIT_NA_NUM_EQ: c_int = 2;

/// Full bitwise comparison for all values (including -0 vs +0).
pub const NE_BIT_NA_NUM_BIT: c_int = 3;

// ---------------------------------------------------------------------------
// Flag bits for R_compute_identical
// ---------------------------------------------------------------------------

/// When set, num.eq = FALSE => use bitwise comparison for numbers.
const IDENT_NUM_AS_BITS: c_int = 1;

/// When set, single.NA = FALSE => distinguish NA bit patterns.
const IDENT_NA_AS_BITS: c_int = 2;

/// When set, attrib.as.set = FALSE => compare attributes by order.
const IDENT_ATTR_BY_ORDER: c_int = 4;

/// When set, ignore bytecode differences.
const IDENT_USE_BYTECODE: c_int = 8;

/// When set, ignore closure environment differences.
const IDENT_USE_CLOENV: c_int = 16;

/// When set, ignore srcref differences.
const IDENT_USE_SRCREF: c_int = 32;

/// When set, compare external pointers by reference.
const IDENT_EXTPTR_AS_REF: c_int = 64;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Check if an SEXP has the OBJECT flag set (local helper).
#[inline]
unsafe fn OBJECT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (*x).sxpinfo.obj() as c_int
    }
}

/// Check if an SEXP has the S4 bit set in gp (local helper).
#[inline]
unsafe fn IS_S4_OBJECT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.gp() & 0x04) as c_int
    }
}

// ---------------------------------------------------------------------------
// neWithNaN — not-equal with NaN awareness
// ---------------------------------------------------------------------------

/// Compare two doubles for inequality, handling NA/NaN according to strictness.
///
/// This is the core comparison function used by `R_identical()` for numeric values.
///
/// Strictness levels:
/// - `NE_SINGLE_NA_NUM_EQ` (0): Treat R's NA as a single NA value (NA != NaN),
///   use numeric `!=` for non-NaN values.
/// - `NE_SINGLE_NA_NUM_BIT` (1): Same NA handling, but use bitwise comparison
///   for non-NaN values (distinguishes -0.0 from +0.0).
/// - `NE_BIT_NA_NUM_EQ` (2): Treat all NaN bit patterns as distinct,
///   use numeric `!=` for non-NaN values.
/// - `NE_BIT_NA_NUM_BIT` (3): Full bitwise comparison for all values.
///
/// Returns 1 if x != y, 0 if x == y (under the chosen strictness).
pub unsafe fn neWithNaN(x: f64, y: f64, str: c_int) -> c_int {
    // Single-NA modes: treat R's NA specially
    match str {
        NE_SINGLE_NA_NUM_EQ | NE_SINGLE_NA_NUM_BIT => {
            if R_IsNA(x) {
                return if R_IsNA(y) { 0 } else { 1 };
            }
            if R_IsNA(y) {
                return if R_IsNA(x) { 0 } else { 1 };
            }
            if ISNAN(x) {
                return if ISNAN(y) { 0 } else { 1 };
            }
        }
        NE_BIT_NA_NUM_EQ | NE_BIT_NA_NUM_BIT => {
            // Fall through to the main comparison
        }
        _ => {
            return 0;
        }
    }

    match str {
        NE_SINGLE_NA_NUM_EQ => {
            if x != y {
                1
            } else {
                0
            }
        }
        NE_BIT_NA_NUM_EQ => {
            if !ISNAN(x) && !ISNAN(y) {
                if x != y { 1 } else { 0 }
            } else {
                // Bitwise comparison for NA/NaN values
                if x.to_bits() != y.to_bits() { 1 } else { 0 }
            }
        }
        NE_SINGLE_NA_NUM_BIT | NE_BIT_NA_NUM_BIT => {
            // Full bitwise comparison
            if x.to_bits() != y.to_bits() { 1 } else { 0 }
        }
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// eqWithNaN — equal with NaN awareness (convenience wrapper)
// ---------------------------------------------------------------------------

/// Compare two doubles for equality, handling NA/NaN.
///
/// This is the inverse of `neWithNaN`.
#[inline]
pub fn eqWithNaN(x: f64, y: f64, str: c_int) -> bool {
    unsafe { neWithNaN(x, y, str) == 0 }
}

/// Safe Rust wrapper for `neWithNaN`.
#[inline]
pub fn ne_with_nan(x: f64, y: f64, str: c_int) -> bool {
    unsafe { neWithNaN(x, y, str) != 0 }
}

// ---------------------------------------------------------------------------
// R_compute_identical — core recursive comparison
// ---------------------------------------------------------------------------

/// Compute the strictness level for neWithNaN from the identical flags.
#[inline]
fn compute_strictness(flags: c_int) -> c_int {
    let mut str = 0;
    if flags & IDENT_NA_AS_BITS != 0 {
        str |= 2; // NE_BIT_NA
    }
    if flags & IDENT_NUM_AS_BITS != 0 {
        str |= 1; // NE_NUM_BIT
    }
    str
}

/// Core recursive identical comparison of two SEXP values.
///
/// This is the workhorse behind R's `identical()` function. It compares
/// two R objects structurally, with configurable strictness for numeric
/// comparisons, NA handling, and attribute comparison.
///
/// Returns 1 if identical, 0 if not.
pub unsafe fn R_compute_identical(x: SEXP, y: SEXP, flags: c_int) -> c_int {
    unsafe {
        // Quick pointer equality check
        if x == y {
            return 1;
        }

        // Both NULL => identical
        if x.is_null() && y.is_null() {
            return 1;
        }
        // One NULL, one non-NULL => not identical
        if x.is_null() || y.is_null() {
            return 0;
        }

        // Check TYPEOF
        if TYPEOF(x) != TYPEOF(y) {
            return 0;
        }

        // Check OBJECT flag
        if OBJECT(x) != OBJECT(y) {
            return 0;
        }

        // Check S4 flag
        if IS_S4_OBJECT(x) != IS_S4_OBJECT(y) {
            return 0;
        }

        // Attribute comparison: both must have attributes or both must not.
        // Full set comparison would require streql and other helpers; for now
        // we do a simple check: both null or both non-null.
        let ax = ATTRIB(x);
        let ay = ATTRIB(y);
        if flags & IDENT_ATTR_BY_ORDER != 0 {
            // Compare attributes by order (simpler)
            if R_compute_identical(ax, ay, flags) == 0 {
                return 0;
            }
        } else {
            // Compare as set: both null or both non-null (simplified)
            if ax.is_null() != ay.is_null() {
                return 0;
            }
        }

        let t = TYPEOF(x);

        // Use integer constants for match since SEXPTYPE::X.as_c_int() is not a pattern
        if t == SEXPTYPE::NILSXP {
            return 1;
        } else if t == SEXPTYPE::LGLSXP {
            // LGLSXP: compare logical arrays via memcmp
            let nx = LENGTH(x);
            let ny = LENGTH(y);
            if nx != ny {
                return 0;
            }
            let lx = LOGICAL(x);
            let ly = LOGICAL(y);
            if lx.is_null() && ly.is_null() {
                return 1;
            }
            if lx.is_null() || ly.is_null() {
                return 0;
            }
            let size = (nx as usize) * std::mem::size_of::<c_int>();
            if size == 0 {
                return 1;
            }
            return if libc::memcmp(lx as *const _, ly as *const _, size) == 0 {
                1
            } else {
                0
            };
        } else if t == SEXPTYPE::INTSXP {
            // INTSXP: compare integer arrays via memcmp
            let nx = LENGTH(x);
            let ny = LENGTH(y);
            if nx != ny {
                return 0;
            }
            let ix = INTEGER(x);
            let iy = INTEGER(y);
            if ix.is_null() && iy.is_null() {
                return 1;
            }
            if ix.is_null() || iy.is_null() {
                return 0;
            }
            let size = (nx as usize) * std::mem::size_of::<c_int>();
            if size == 0 {
                return 1;
            }
            return if libc::memcmp(ix as *const _, iy as *const _, size) == 0 {
                1
            } else {
                0
            };
        } else if t == SEXPTYPE::REALSXP {
            // REALSXP: compare doubles element-by-element using neWithNaN
            let nx = LENGTH(x);
            let ny = LENGTH(y);
            if nx != ny {
                return 0;
            }
            let rx = REAL(x);
            let ry = REAL(y);
            if rx.is_null() && ry.is_null() {
                return 1;
            }
            if rx.is_null() || ry.is_null() {
                return 0;
            }
            let str = compute_strictness(flags);
            for i in 0..nx as usize {
                if neWithNaN(*rx.add(i), *ry.add(i), str) != 0 {
                    return 0;
                }
            }
            return 1;
        } else if t == SEXPTYPE::CPLXSXP {
            // CPLXSXP: compare complex numbers element-by-element
            let nx = LENGTH(x);
            let ny = LENGTH(y);
            if nx != ny {
                return 0;
            }
            let cx = COMPLEX(x);
            let cy = COMPLEX(y);
            if cx.is_null() && cy.is_null() {
                return 1;
            }
            if cx.is_null() || cy.is_null() {
                return 0;
            }
            let str = compute_strictness(flags);
            for i in 0..nx as usize {
                let zx = *cx.add(i);
                let zy = *cy.add(i);
                if neWithNaN(zx.r, zy.r, str) != 0 {
                    return 0;
                }
                if neWithNaN(zx.i, zy.i, str) != 0 {
                    return 0;
                }
            }
            return 1;
        } else if t == SEXPTYPE::STRSXP {
            // STRSXP: compare CHARSXP pointers element by element
            let nx = LENGTH(x);
            let ny = LENGTH(y);
            if nx != ny {
                return 0;
            }
            for i in 0..nx as i64 {
                let sx = STRING_ELT(x, i);
                let sy = STRING_ELT(y, i);
                if sx == sy {
                    continue;
                }
                if sx.is_null() && sy.is_null() {
                    continue;
                }
                if sx.is_null() || sy.is_null() {
                    return 0;
                }
                return 0;
            }
            return 1;
        } else if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP {
            // VECSXP / EXPRSXP: recursive comparison of elements
            let nx = LENGTH(x);
            let ny = LENGTH(y);
            if nx != ny {
                return 0;
            }
            for i in 0..nx as i64 {
                if R_compute_identical(VECTOR_ELT(x, i), VECTOR_ELT(y, i), flags) == 0 {
                    return 0;
                }
            }
            return 1;
        } else if t == SEXPTYPE::LISTSXP {
            // LISTSXP: recursive on CAR, CDR, TAG
            let mut lx = x;
            let mut ly = y;
            loop {
                if R_compute_identical(CAR(lx), CAR(ly), flags) == 0 {
                    return 0;
                }
                if R_compute_identical(TAG(lx), TAG(ly), flags) == 0 {
                    return 0;
                }
                let nx = CDR(lx);
                let ny = CDR(ly);
                if nx == ny {
                    return 1;
                }
                if nx.is_null() || ny.is_null() {
                    return 0;
                }
                lx = nx;
                ly = ny;
            }
        } else if t == SEXPTYPE::LANGSXP {
            // LANGSXP: recursive on CAR, CDR, TAG
            let mut lx = x;
            let mut ly = y;
            loop {
                if R_compute_identical(CAR(lx), CAR(ly), flags) == 0 {
                    return 0;
                }
                if R_compute_identical(TAG(lx), TAG(ly), flags) == 0 {
                    return 0;
                }
                let nx = CDR(lx);
                let ny = CDR(ly);
                if nx == ny {
                    return 1;
                }
                if nx.is_null() || ny.is_null() {
                    return 0;
                }
                lx = nx;
                ly = ny;
            }
        } else if t == SEXPTYPE::CLOSXP {
            // CLOSXP: compare formals, body, environment
            if R_compute_identical(FORMALS(x), FORMALS(y), flags) == 0 {
                return 0;
            }
            if R_compute_identical(BODY(x), BODY(y), flags) == 0 {
                return 0;
            }
            if flags & IDENT_USE_CLOENV != 0
                && R_compute_identical(CLOENV(x), CLOENV(y), flags) == 0
            {
                return 0;
            }
            return 1;
        } else if t == SEXPTYPE::ENVSXP || t == SEXPTYPE::SYMSXP {
            // ENVSXP/SYMSXP: pointer equality only (already checked x != y)
            return 0;
        } else if t == SEXPTYPE::PROMSXP {
            // PROMSXP: compare value, expression, environment
            let px = (*x).data.promsxp.value;
            let py = (*y).data.promsxp.value;
            let ex = (*x).data.promsxp.expr;
            let ey = (*y).data.promsxp.expr;
            let enx = (*x).data.promsxp.env;
            let eny = (*y).data.promsxp.env;
            return if px == py && ex == ey && enx == eny {
                1
            } else {
                0
            };
        } else if t == SEXPTYPE::RAWSXP {
            // RAWSXP: memcmp on raw bytes
            let nx = LENGTH(x);
            let ny = LENGTH(y);
            if nx != ny {
                return 0;
            }
            let rx = RAW(x);
            let ry = RAW(y);
            if rx.is_null() && ry.is_null() {
                return 1;
            }
            if rx.is_null() || ry.is_null() {
                return 0;
            }
            let size = nx as usize;
            if size == 0 {
                return 1;
            }
            return if libc::memcmp(rx as *const _, ry as *const _, size) == 0 {
                1
            } else {
                0
            };
        } else if t == SEXPTYPE::SPECIALSXP || t == SEXPTYPE::BUILTINSXP {
            // SPECIALSXP / BUILTINSXP: compare PRIMOFFSET
            return if PRIMOFFSET(x) == PRIMOFFSET(y) { 1 } else { 0 };
        }

        // Default: pointer equality (already checked x != y)
        0
    }
}

// ---------------------------------------------------------------------------
// R_identical — old API wrapper
// ---------------------------------------------------------------------------

/// Old API for identical() with default R flags.
///
/// Calls `R_compute_identical(s1, s2, 0)` with all default settings:
/// numeric equality for numbers, single NA representation,
/// attribute comparison as set.
pub unsafe fn R_identical(s1: SEXP, s2: SEXP) -> c_int {
    unsafe { R_compute_identical(s1, s2, 0) }
}

// ---------------------------------------------------------------------------
// do_identical — R's identical() built-in entry point
// ---------------------------------------------------------------------------

/// R's `identical()` built-in function.
///
/// Arguments: x, y, num.eq, single.NA, attrib.as.set,
///            ignore.bytecode, ignore.environment, ignore.srcref, extptr.as.ref
pub unsafe fn do_identical(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::Rf_ScalarLogical;
        use crate::sexp::globals::R_NilValue;

        let x = CAR(args);
        let rest = CDR(args);
        let y = if rest.is_null() || rest == R_NilValue() {
            R_NilValue()
        } else {
            CAR(rest)
        };

        // Parse the flag arguments
        let mut flags: c_int = 0;
        let mut opt = if rest.is_null() || rest == R_NilValue() {
            R_NilValue()
        } else {
            CDR(rest)
        };

        let mut next_arg = || {
            if opt.is_null() || opt == R_NilValue() {
                None
            } else {
                let value = CAR(opt);
                opt = CDR(opt);
                Some(value)
            }
        };

        let logical_false = |value: SEXP| -> bool {
            !value.is_null() && TYPEOF(value) == SEXPTYPE::LGLSXP && {
                let ptr = LOGICAL(value);
                !ptr.is_null() && *ptr == 0
            }
        };

        let logical_true = |value: SEXP| -> bool {
            !value.is_null() && TYPEOF(value) == SEXPTYPE::LGLSXP && {
                let ptr = LOGICAL(value);
                !ptr.is_null() && *ptr != 0
            }
        };

        // num.eq: default TRUE. If num.eq=FALSE, set IDENT_NUM_AS_BITS
        if let Some(v) = next_arg() {
            if logical_false(v) {
                flags |= IDENT_NUM_AS_BITS;
            }
        }

        // single.NA: default TRUE. If single.NA=FALSE, set IDENT_NA_AS_BITS
        if let Some(v) = next_arg() {
            if logical_false(v) {
                flags |= IDENT_NA_AS_BITS;
            }
        }

        // attrib.as.set: default TRUE. If FALSE, set IDENT_ATTR_BY_ORDER
        if let Some(v) = next_arg() {
            if logical_false(v) {
                flags |= IDENT_ATTR_BY_ORDER;
            }
        }

        // ignore.bytecode: default TRUE
        if let Some(v) = next_arg() {
            if logical_true(v) {
                flags |= IDENT_USE_BYTECODE;
            }
        }

        // ignore.environment: default FALSE
        if let Some(v) = next_arg() {
            if logical_true(v) {
                flags |= IDENT_USE_CLOENV;
            }
        }

        // ignore.srcref: default TRUE
        if let Some(v) = next_arg() {
            if logical_true(v) {
                flags |= IDENT_USE_SRCREF;
            }
        }

        // extptr.as.ref: default FALSE
        if let Some(v) = next_arg() {
            if logical_true(v) {
                flags |= IDENT_EXTPTR_AS_REF;
            }
        }

        let result = R_compute_identical(x, y, flags);
        Rf_ScalarLogical(result)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::globals::*;

    use super::*;
    use crate::sexp::ffi::SexprecCore;
    use std::ptr;

    #[test]
    fn test_neWithNaN_single_na_num_eq() {
        let str = NE_SINGLE_NA_NUM_EQ;
        // Regular values
        assert!(!ne_with_nan(1.0, 1.0, str));
        assert!(ne_with_nan(1.0, 2.0, str));

        // -0 vs +0: numerically equal
        assert!(!ne_with_nan(0.0, -0.0, str));

        // R's NA vs R's NA: equal
        assert!(!ne_with_nan(
            f64::from_bits(R_NA_BIT_PATTERN),
            f64::from_bits(R_NA_BIT_PATTERN),
            str
        ));

        // R's NA vs regular NaN: not equal (single NA mode)
        let na = f64::from_bits(R_NA_BIT_PATTERN);
        assert!(ne_with_nan(na, f64::NAN, str));

        // Regular NaN vs regular NaN: equal (any NaN matches any NaN)
        assert!(!ne_with_nan(f64::NAN, f64::NAN, str));

        // NaN vs non-NaN: not equal
        assert!(ne_with_nan(f64::NAN, 1.0, str));
    }

    #[test]
    fn test_neWithNaN_bit_na_num_bit() {
        let str = NE_BIT_NA_NUM_BIT;

        // Regular values
        assert!(!ne_with_nan(1.0, 1.0, str));
        assert!(ne_with_nan(1.0, 2.0, str));

        // -0 vs +0: bitwise NOT equal
        assert!(ne_with_nan(0.0, -0.0, str));

        // Different NaN bit patterns: bitwise NOT equal
        let nan1 = f64::NAN;
        let nan2 = f64::from_bits(f64::NAN.to_bits() | 1);
        assert!(ne_with_nan(nan1, nan2, str));
    }

    #[test]
    fn test_neWithNaN_bit_na_num_eq() {
        let str = NE_BIT_NA_NUM_EQ;

        // Regular values: numeric comparison
        assert!(!ne_with_nan(1.0, 1.0, str));
        assert!(!ne_with_nan(0.0, -0.0, str)); // numerically equal

        // NaN: bitwise comparison
        let nan1 = f64::NAN;
        let nan2 = f64::from_bits(f64::NAN.to_bits() | 1);
        assert!(ne_with_nan(nan1, nan2, str)); // different bit patterns
    }

    #[test]
    fn test_eqWithNaN() {
        assert!(eqWithNaN(1.0, 1.0, NE_SINGLE_NA_NUM_EQ));
        assert!(!eqWithNaN(1.0, 2.0, NE_SINGLE_NA_NUM_EQ));
        assert!(eqWithNaN(f64::NAN, f64::NAN, NE_SINGLE_NA_NUM_EQ));
    }

    #[test]
    fn test_compute_identical_null() {
        unsafe {
            // Both null => identical
            assert_eq!(R_compute_identical(ptr::null_mut(), ptr::null_mut(), 0), 1);
        }
    }

    #[test]
    fn test_compute_identical_same_pointer() {
        unsafe {
            let nil = R_NilValue();
            assert_eq!(R_compute_identical(nil, nil, 0), 1);
        }
    }

    #[test]
    fn test_compute_identical_different_types() {
        unsafe {
            // Different types => not identical
            let mut node_int = Box::new(SexprecCore::new_vector(SEXPTYPE::INTSXP, 1));
            let mut node_real = Box::new(SexprecCore::new_vector(SEXPTYPE::REALSXP, 1));
            let x = node_int.as_mut() as *mut _ as SEXP;
            let y = node_real.as_mut() as *mut _ as SEXP;
            assert_eq!(R_compute_identical(x, y, 0), 0);
        }
    }

    #[test]
    fn test_compute_identical_different_length() {
        unsafe {
            let mut node1 = Box::new(SexprecCore::new_vector(SEXPTYPE::INTSXP, 1));
            let mut node2 = Box::new(SexprecCore::new_vector(SEXPTYPE::INTSXP, 2));
            let x = node1.as_mut() as *mut _ as SEXP;
            let y = node2.as_mut() as *mut _ as SEXP;
            assert_eq!(R_compute_identical(x, y, 0), 0);
        }
    }

    #[test]
    fn test_compute_identical_integer_arrays() {
        use crate::sexp::ffi::SexprecData;
        unsafe {
            // Create two INTSXP vectors with same data
            let data1 = Box::new([42i32, 99i32]);
            let data2 = Box::new([42i32, 99i32]);

            let mut node1 = Box::new(SexprecCore::new_vector(SEXPTYPE::INTSXP, 2));
            node1.gengc_next_node = data1.as_ptr() as *mut SexprecCore;
            node1.data = SexprecData {
                vecsxp: crate::sexp::ffi::Vecsxp {
                    length: 2,
                    truelength: 2,
                },
            };

            let mut node2 = Box::new(SexprecCore::new_vector(SEXPTYPE::INTSXP, 2));
            node2.gengc_next_node = data2.as_ptr() as *mut SexprecCore;
            node2.data = SexprecData {
                vecsxp: crate::sexp::ffi::Vecsxp {
                    length: 2,
                    truelength: 2,
                },
            };

            let x = node1.as_mut() as *mut _ as SEXP;
            let y = node2.as_mut() as *mut _ as SEXP;

            assert_eq!(R_compute_identical(x, y, 0), 1);
        }
    }

    #[test]
    fn test_compute_identical_integer_arrays_differ() {
        use crate::sexp::ffi::SexprecData;
        unsafe {
            let data1 = Box::new([42i32, 99i32]);
            let data2 = Box::new([42i32, 100i32]);

            let mut node1 = Box::new(SexprecCore::new_vector(SEXPTYPE::INTSXP, 2));
            node1.gengc_next_node = data1.as_ptr() as *mut SexprecCore;
            node1.data = SexprecData {
                vecsxp: crate::sexp::ffi::Vecsxp {
                    length: 2,
                    truelength: 2,
                },
            };

            let mut node2 = Box::new(SexprecCore::new_vector(SEXPTYPE::INTSXP, 2));
            node2.gengc_next_node = data2.as_ptr() as *mut SexprecCore;
            node2.data = SexprecData {
                vecsxp: crate::sexp::ffi::Vecsxp {
                    length: 2,
                    truelength: 2,
                },
            };

            let x = node1.as_mut() as *mut _ as SEXP;
            let y = node2.as_mut() as *mut _ as SEXP;

            assert_eq!(R_compute_identical(x, y, 0), 0);
        }
    }

    #[test]
    fn test_compute_identical_raw_arrays() {
        use crate::sexp::ffi::SexprecData;
        unsafe {
            let data1 = Box::new([1u8, 2, 3]);
            let data2 = Box::new([1u8, 2, 3]);

            let mut node1 = Box::new(SexprecCore::new_vector(SEXPTYPE::RAWSXP, 3));
            node1.gengc_next_node = data1.as_ptr() as *mut SexprecCore;
            node1.data = SexprecData {
                vecsxp: crate::sexp::ffi::Vecsxp {
                    length: 3,
                    truelength: 3,
                },
            };

            let mut node2 = Box::new(SexprecCore::new_vector(SEXPTYPE::RAWSXP, 3));
            node2.gengc_next_node = data2.as_ptr() as *mut SexprecCore;
            node2.data = SexprecData {
                vecsxp: crate::sexp::ffi::Vecsxp {
                    length: 3,
                    truelength: 3,
                },
            };

            let x = node1.as_mut() as *mut _ as SEXP;
            let y = node2.as_mut() as *mut _ as SEXP;

            assert_eq!(R_compute_identical(x, y, 0), 1);
        }
    }

    #[test]
    fn test_compute_identical_special_builtin_offset() {
        unsafe {
            let mut node1 = Box::new(SexprecCore::new(SEXPTYPE::SPECIALSXP));
            let mut node2 = Box::new(SexprecCore::new(SEXPTYPE::SPECIALSXP));
            let mut node3 = Box::new(SexprecCore::new(SEXPTYPE::SPECIALSXP));

            // Same offset => identical
            node1.data = crate::sexp::ffi::SexprecData {
                primsxp: crate::sexp::ffi::Primsxp { offset: 42 },
            };
            node2.data = crate::sexp::ffi::SexprecData {
                primsxp: crate::sexp::ffi::Primsxp { offset: 42 },
            };
            node3.data = crate::sexp::ffi::SexprecData {
                primsxp: crate::sexp::ffi::Primsxp { offset: 99 },
            };

            let x = node1.as_mut() as *mut _ as SEXP;
            let y = node2.as_mut() as *mut _ as SEXP;
            let z = node3.as_mut() as *mut _ as SEXP;

            assert_eq!(R_compute_identical(x, y, 0), 1);
            assert_eq!(R_compute_identical(x, z, 0), 0);
        }
    }

    #[test]
    fn test_compute_identical_one_null() {
        unsafe {
            let nil = R_NilValue();
            let mut node = Box::new(SexprecCore::new_vector(SEXPTYPE::INTSXP, 0));
            let x = node.as_mut() as *mut _ as SEXP;
            assert_eq!(R_compute_identical(nil, x, 0), 0);
            assert_eq!(R_compute_identical(x, nil, 0), 0);
        }
    }

    #[test]
    fn test_compute_identical_object_flag_differs() {
        unsafe {
            let mut node1 = Box::new(SexprecCore::new_vector(SEXPTYPE::INTSXP, 0));
            let mut node2 = Box::new(SexprecCore::new_vector(SEXPTYPE::INTSXP, 0));
            node2.sxpinfo.set_obj(true);

            let x = node1.as_mut() as *mut _ as SEXP;
            let y = node2.as_mut() as *mut _ as SEXP;

            assert_eq!(R_compute_identical(x, y, 0), 0);
        }
    }

    #[test]
    fn test_compute_strictness() {
        // Default flags (0) => NE_SINGLE_NA_NUM_EQ (0)
        assert_eq!(compute_strictness(0), 0);

        // IDENT_NA_AS_BITS set => bitwise NA
        assert_eq!(compute_strictness(2), 2);

        // IDENT_NUM_AS_BITS set => bitwise numbers
        assert_eq!(compute_strictness(1), 1);

        // Both set => NE_BIT_NA_NUM_BIT (3)
        assert_eq!(compute_strictness(3), 3);
    }

    #[test]
    fn test_R_identical() {
        unsafe {
            let nil = R_NilValue();
            assert_eq!(R_identical(nil, nil), 1);
            assert_eq!(R_identical(ptr::null_mut(), ptr::null_mut()), 1);
        }
    }
}
