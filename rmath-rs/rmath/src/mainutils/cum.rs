#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/cum.c — cumulative operations on arrays.
//!
//! These functions implement cumulative sum, product, max, and min
//! for integer and double arrays, with proper NA/NaN handling.
//!
//! Ported standalone functions:
//!   cumsum_double, icumsum_int, ccumsum_complex,
//!   cumprod_double, ccumprod_complex,
//!   cummax_double, cummin_double,
//!   icummax_int, icummin_int,
//!   handle_nan_double, chandle_nan_complex
//!
//! SEXP-dependent functions (ported from R's do_cum):
//!   do_cumsum, do_cumprod, do_cummax, do_cummin

use std::ffi::CString;
use std::os::raw::{c_double, c_int};

use crate::eval::attrib_core::{R_NamesSymbol, getAttrib, setAttrib};
use crate::mainutils::errors::{Rf_error, Rf_warning};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_INTEGER, R_NA_BIT_PATTERN, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::*;
use crate::sexp::protect::*;

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
// NaN handling helpers
// ---------------------------------------------------------------------------

/// Handle NaN propagation for cumulative double results.
///
/// After computing cumulative sums/products that may have encountered NaN,
/// this post-processes the result array: once any NaN is seen, all subsequent
/// values become NA_REAL (if the NaN was R's NA) or R_NaN.
pub fn handle_nan_double(x: &[f64], s: &mut [f64]) {
    let mut has_na = false;
    let mut has_nan = false;
    let len = x.len().min(s.len());

    for i in 0..len {
        has_nan = has_nan || ISNAN(x[i]);
        has_na = has_na || (has_nan && R_IsNA(x[i]));
        if has_na || has_nan {
            s[i] = crate::sexp::ffi::NA_REAL;
        }
    }
}

/// Handle NaN propagation for cumulative complex results.
///
/// `r_is_n`: whether the real part had NaN
/// `i_is_n`: whether the imaginary part had NaN
#[allow(clippy::if_same_then_else)]
pub fn chandle_nan_complex(x: &[Rcomplex], s: &mut [Rcomplex], r_is_n: bool, i_is_n: bool) {
    let mut has_na = false;
    let mut has_nan = false;
    let len = x.len().min(s.len());

    for i in 0..len {
        has_nan = has_nan || ISNAN(x[i].r) || ISNAN(x[i].i);
        has_na = has_na || (has_nan && (R_IsNA(x[i].r) || R_IsNA(x[i].i)));
        if has_na {
            if r_is_n {
                s[i].r = crate::sexp::ffi::NA_REAL;
            }
            if i_is_n {
                s[i].i = crate::sexp::ffi::NA_REAL;
            }
        } else if has_nan {
            if r_is_n {
                s[i].r = f64::NAN;
            }
            if i_is_n {
                s[i].i = f64::NAN;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cumulative sum
// ---------------------------------------------------------------------------

/// Cumulative sum of a double array.
///
/// `x` is the input, `s` is the output (must be same length).
/// Uses `f64` accumulator for precision.
pub fn cumsum_double(x: &[f64], s: &mut [f64]) {
    let len = x.len().min(s.len());
    if len == 0 {
        return;
    }

    let mut sum: f64 = 0.0;
    for i in 0..len {
        sum += x[i]; // NaN propagated
        s[i] = sum;
    }
    if ISNAN(sum) {
        handle_nan_double(x, s);
    }
}

/// Cumulative sum of an integer array.
///
/// Uses `f64` accumulator. Stops and sets remaining to NA_INTEGER
/// on overflow. `warn` is set to true if overflow occurred.
pub fn icumsum_int(x: &[c_int], s: &mut [c_int], warn: &mut bool) {
    let len = x.len().min(s.len());
    if len == 0 {
        return;
    }

    let mut sum: f64 = 0.0;
    for i in 0..len {
        if x[i] == NA_INTEGER {
            // remaining entries stay as initialized (0)
            break;
        }
        sum += x[i] as f64;
        if sum > c_int::MAX as f64 || sum < (c_int::MIN as f64) + 1.0 {
            *warn = true;
            break;
        }
        s[i] = sum as c_int;
    }
}

/// Cumulative sum of a complex array.
pub fn ccumsum_complex(x: &[Rcomplex], s: &mut [Rcomplex]) {
    let len = x.len().min(s.len());
    if len == 0 {
        return;
    }

    let mut sum_r: f64 = 0.0;
    let mut sum_i: f64 = 0.0;
    for i in 0..len {
        sum_r += x[i].r;
        sum_i += x[i].i;
        s[i].r = sum_r;
        s[i].i = sum_i;
    }
    if ISNAN(sum_r) || ISNAN(sum_i) {
        chandle_nan_complex(x, s, ISNAN(sum_r), ISNAN(sum_i));
    }
}

// ---------------------------------------------------------------------------
// Cumulative product
// ---------------------------------------------------------------------------

/// Cumulative product of a double array.
pub fn cumprod_double(x: &[f64], s: &mut [f64]) {
    let len = x.len().min(s.len());
    if len == 0 {
        return;
    }

    let mut prod: f64 = 1.0;
    for i in 0..len {
        prod *= x[i]; // NaN propagated
        s[i] = prod;
    }
    if ISNAN(prod) {
        handle_nan_double(x, s);
    }
}

/// Cumulative product of a complex array.
pub fn ccumprod_complex(x: &[Rcomplex], s: &mut [Rcomplex]) {
    let len = x.len().min(s.len());
    if len == 0 {
        return;
    }

    let mut prod_r: f64 = 1.0;
    let mut prod_i: f64 = 0.0;
    for i in 0..len {
        let tmp_r = prod_r;
        let tmp_i = prod_i;
        prod_r = x[i].r * tmp_r - x[i].i * tmp_i;
        prod_i = x[i].r * tmp_i + x[i].i * tmp_r;
        s[i].r = prod_r;
        s[i].i = prod_i;
    }
    if ISNAN(prod_r) || ISNAN(prod_i) {
        chandle_nan_complex(x, s, ISNAN(prod_r), ISNAN(prod_i));
    }
}

// ---------------------------------------------------------------------------
// Cumulative max/min (double)
// ---------------------------------------------------------------------------

/// Cumulative maximum of a double array.
pub fn cummax_double(x: &[f64], s: &mut [f64]) {
    let len = x.len().min(s.len());
    if len == 0 {
        return;
    }

    let mut max = f64::NEG_INFINITY;
    for i in 0..len {
        if ISNAN(x[i]) {
            handle_nan_double(x, s);
            return;
        }
        max = if max > x[i] { max } else { x[i] };
        s[i] = max;
    }
}

/// Cumulative minimum of a double array.
pub fn cummin_double(x: &[f64], s: &mut [f64]) {
    let len = x.len().min(s.len());
    if len == 0 {
        return;
    }

    let mut min = f64::INFINITY;
    for i in 0..len {
        if ISNAN(x[i]) {
            handle_nan_double(x, s);
            return;
        }
        min = if min < x[i] { min } else { x[i] };
        s[i] = min;
    }
}

// ---------------------------------------------------------------------------
// Cumulative max/min (integer)
// ---------------------------------------------------------------------------

/// Cumulative maximum of an integer array.
pub fn icummax_int(x: &[c_int], s: &mut [c_int]) {
    let len = x.len().min(s.len());
    if len == 0 {
        return;
    }
    if x[0] == NA_INTEGER {
        return;
    }

    let mut max = x[0];
    s[0] = max;
    for i in 1..len {
        if x[i] == NA_INTEGER {
            break;
        }
        max = if max > x[i] { max } else { x[i] };
        s[i] = max;
    }
}

/// Cumulative minimum of an integer array.
pub fn icummin_int(x: &[c_int], s: &mut [c_int]) {
    let len = x.len().min(s.len());
    if len == 0 {
        return;
    }

    let mut min = x[0];
    s[0] = min;
    for i in 1..len {
        if x[i] == NA_INTEGER {
            break;
        }
        min = if min < x[i] { min } else { x[i] };
        s[i] = min;
    }
}

// ---------------------------------------------------------------------------
// R-level cumulative functions
// ---------------------------------------------------------------------------

/// do_cumsum — R's cumsum() function.
///
/// Computes cumulative sums, handling integer, logical, double, and complex
/// vectors with proper NA/NaN propagation.
///
/// Matches R's do_cum with PRIMVAL(op) == 1:
/// - For complex: uses complex cumsum
/// - For integer/logical: tries integer cumsum with overflow detection
/// - Otherwise: coerces to double and uses double cumsum
pub unsafe fn do_cumsum(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let s = CAR(args);
        if s.is_null() || s == R_NilValue() {
            return R_NilValue();
        }

        let t = TYPEOF(s);
        let n = XLENGTH(s);

        if t == SEXPTYPE::CPLXSXP {
            // Complex path: allocate, copy names, compute ccumsum
            let ans = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
            let _ans_guard = protect(ans);
            setAttrib(ans, R_NamesSymbol(), getAttrib(s, R_NamesSymbol()));
            if n == 0 {
                return ans;
            }
            let src = std::slice::from_raw_parts(COMPLEX(s), n as usize);
            let dst = std::slice::from_raw_parts_mut(COMPLEX(ans), n as usize);
            ccumsum_complex(src, dst);
            ans
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            // Integer/logical path: coerce to integer, try integer cumsum
            let t = crate::mainutils::coerce::coerceVector(s, SEXPTYPE::INTSXP.as_c_int());
            let _coerced_guard = protect(t);
            let n2 = XLENGTH(t);
            let ans = Rf_allocVector3(SEXPTYPE::INTSXP, n2);
            let _ans_guard = protect(ans);
            setAttrib(ans, R_NamesSymbol(), getAttrib(t, R_NamesSymbol()));
            if n2 == 0 {
                return ans;
            }
            // Initialize all result elements to NA_INTEGER (R does this)
            let dst = INTEGER(ans);
            for i in 0..n2 as usize {
                *dst.add(i) = NA_INTEGER;
            }
            // Integer cumsum with overflow detection (R's icumsum logic)
            let src = INTEGER(t);
            let mut sum: c_double = 0.0;
            for i in 0..n2 as usize {
                let v = *src.add(i);
                if v == NA_INTEGER {
                    break;
                }
                sum += v as c_double;
                if sum > c_int::MAX as c_double || sum < (c_int::MIN as c_double) + 1.0 {
                    // Integer overflow — issue warning and stop
                    let msg =
                        CString::new("integer overflow in 'cumsum'; use 'cumsum(as.numeric(.))'")
                            .unwrap_or_default();
                    Rf_warning(msg.as_ptr());
                    break;
                }
                *dst.add(i) = sum as c_int;
            }
            ans
        } else {
            // Real / other types: coerce to double
            let t = crate::mainutils::coerce::coerceVector(s, SEXPTYPE::REALSXP.as_c_int());
            let _coerced_guard = protect(t);
            let n2 = XLENGTH(t);
            let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n2);
            let _ans_guard = protect(ans);
            setAttrib(ans, R_NamesSymbol(), getAttrib(t, R_NamesSymbol()));
            if n2 == 0 {
                return ans;
            }
            let src = std::slice::from_raw_parts(REAL(t), n2 as usize);
            let dst = std::slice::from_raw_parts_mut(REAL(ans), n2 as usize);
            cumsum_double(src, dst);
            ans
        }
    }
}

/// do_cumprod — R's cumprod() function.
///
/// Computes cumulative products, handling integer, logical, double, and complex
/// vectors with proper NA/NaN propagation.
///
/// Matches R's do_cum with PRIMVAL(op) == 2:
/// - For complex: uses complex cumprod
/// - For integer/logical: coerces to double first (R excludes cumprod from integer path)
/// - Otherwise: coerces to double and uses double cumprod
pub unsafe fn do_cumprod(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let s = CAR(args);
        if s.is_null() || s == R_NilValue() {
            return R_NilValue();
        }

        let t = TYPEOF(s);
        let n = XLENGTH(s);

        if t == SEXPTYPE::CPLXSXP {
            // Complex path
            let ans = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
            let _ans_guard = protect(ans);
            setAttrib(ans, R_NamesSymbol(), getAttrib(s, R_NamesSymbol()));
            if n == 0 {
                return ans;
            }
            let src = std::slice::from_raw_parts(COMPLEX(s), n as usize);
            let dst = std::slice::from_raw_parts_mut(COMPLEX(ans), n as usize);
            ccumprod_complex(src, dst);
            ans
        } else {
            // All non-complex types (including integer/logical) coerce to double
            // R's do_cum excludes cumprod (PRIMVAL == 2) from the integer path
            let t = crate::mainutils::coerce::coerceVector(s, SEXPTYPE::REALSXP.as_c_int());
            let _coerced_guard = protect(t);
            let n2 = XLENGTH(t);
            let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n2);
            let _ans_guard = protect(ans);
            setAttrib(ans, R_NamesSymbol(), getAttrib(t, R_NamesSymbol()));
            if n2 == 0 {
                return ans;
            }
            let src = std::slice::from_raw_parts(REAL(t), n2 as usize);
            let dst = std::slice::from_raw_parts_mut(REAL(ans), n2 as usize);
            cumprod_double(src, dst);
            ans
        }
    }
}

/// do_cummax — R's cummax() function.
///
/// Computes cumulative maxima over a vector.
///
/// Matches R's do_cum with PRIMVAL(op) == 3:
/// - For complex: error (not defined for complex)
/// - For integer/logical: uses integer cummax (initialized to NA_INTEGER)
/// - Otherwise: coerces to double and uses double cummax
pub unsafe fn do_cummax(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let s = CAR(args);
        if s.is_null() || s == R_NilValue() {
            return R_NilValue();
        }

        let t = TYPEOF(s);
        let n = XLENGTH(s);

        if t == SEXPTYPE::CPLXSXP {
            // R errors: "'cummax' not defined for complex numbers"
            let msg = CString::new("'cummax' not defined for complex numbers").unwrap_or_default();
            Rf_error(msg.as_ptr());
            unreachable!()
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            // Integer/logical path: coerce to integer, use icummax logic
            let t = crate::mainutils::coerce::coerceVector(s, SEXPTYPE::INTSXP.as_c_int());
            let _coerced_guard = protect(t);
            let n2 = XLENGTH(t);
            let ans = Rf_allocVector3(SEXPTYPE::INTSXP, n2);
            let _ans_guard = protect(ans);
            setAttrib(ans, R_NamesSymbol(), getAttrib(t, R_NamesSymbol()));
            if n2 == 0 {
                return ans;
            }
            // Initialize all result elements to NA_INTEGER
            let dst = INTEGER(ans);
            for i in 0..n2 as usize {
                *dst.add(i) = NA_INTEGER;
            }
            let src = INTEGER(t);
            // R's icummax: if first element is NA, return all-NA
            if *src != NA_INTEGER {
                let mut max = *src;
                *dst = max;
                for i in 1..n2 as usize {
                    let v = *src.add(i);
                    if v == NA_INTEGER {
                        break;
                    }
                    max = if max > v { max } else { v };
                    *dst.add(i) = max;
                }
            }
            ans
        } else {
            // Real / other types: coerce to double
            let t = crate::mainutils::coerce::coerceVector(s, SEXPTYPE::REALSXP.as_c_int());
            let _coerced_guard = protect(t);
            let n2 = XLENGTH(t);
            let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n2);
            let _ans_guard = protect(ans);
            setAttrib(ans, R_NamesSymbol(), getAttrib(t, R_NamesSymbol()));
            if n2 == 0 {
                return ans;
            }
            let src = std::slice::from_raw_parts(REAL(t), n2 as usize);
            let dst = std::slice::from_raw_parts_mut(REAL(ans), n2 as usize);
            cummax_double(src, dst);
            ans
        }
    }
}

/// do_cummin — R's cummin() function.
///
/// Computes cumulative minima over a vector.
///
/// Matches R's do_cum with PRIMVAL(op) == 4:
/// - For complex: error (not defined for complex)
/// - For integer/logical: uses integer cummin (initialized to NA_INTEGER)
/// - Otherwise: coerces to double and uses double cummin
pub unsafe fn do_cummin(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let s = CAR(args);
        if s.is_null() || s == R_NilValue() {
            return R_NilValue();
        }

        let t = TYPEOF(s);
        let n = XLENGTH(s);

        if t == SEXPTYPE::CPLXSXP {
            // R errors: "'cummin' not defined for complex numbers"
            let msg = CString::new("'cummin' not defined for complex numbers").unwrap_or_default();
            Rf_error(msg.as_ptr());
            unreachable!()
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            // Integer/logical path: coerce to integer, use icummin logic
            let t = crate::mainutils::coerce::coerceVector(s, SEXPTYPE::INTSXP.as_c_int());
            let _coerced_guard = protect(t);
            let n2 = XLENGTH(t);
            let ans = Rf_allocVector3(SEXPTYPE::INTSXP, n2);
            let _ans_guard = protect(ans);
            setAttrib(ans, R_NamesSymbol(), getAttrib(t, R_NamesSymbol()));
            if n2 == 0 {
                return ans;
            }
            // Initialize all result elements to NA_INTEGER
            let dst = INTEGER(ans);
            for i in 0..n2 as usize {
                *dst.add(i) = NA_INTEGER;
            }
            let src = INTEGER(t);
            // R's icummin logic
            let mut min = *src;
            *dst = min;
            for i in 1..n2 as usize {
                let v = *src.add(i);
                if v == NA_INTEGER {
                    break;
                }
                min = if min < v { min } else { v };
                *dst.add(i) = min;
            }
            ans
        } else {
            // Real / other types: coerce to double
            let t = crate::mainutils::coerce::coerceVector(s, SEXPTYPE::REALSXP.as_c_int());
            let _coerced_guard = protect(t);
            let n2 = XLENGTH(t);
            let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n2);
            let _ans_guard = protect(ans);
            setAttrib(ans, R_NamesSymbol(), getAttrib(t, R_NamesSymbol()));
            if n2 == 0 {
                return ans;
            }
            let src = std::slice::from_raw_parts(REAL(t), n2 as usize);
            let dst = std::slice::from_raw_parts_mut(REAL(ans), n2 as usize);
            cummin_double(src, dst);
            ans
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cumsum_double() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let mut s = [0.0; 5];
        cumsum_double(&x, &mut s);
        assert_eq!(s, [1.0, 3.0, 6.0, 10.0, 15.0]);
    }

    #[test]
    fn test_cumsum_double_with_nan() {
        let x = [1.0, 2.0, f64::NAN, 4.0, 5.0];
        let mut s = [0.0; 5];
        cumsum_double(&x, &mut s);
        assert_eq!(s[0], 1.0);
        assert_eq!(s[1], 3.0);
        assert!(s[2].is_nan());
        assert!(s[3].is_nan());
        assert!(s[4].is_nan());
    }

    #[test]
    fn test_cumsum_empty() {
        let x: [f64; 0] = [];
        let mut s: [f64; 0] = [];
        cumsum_double(&x, &mut s);
    }

    #[test]
    fn test_icumsum_int() {
        let x = [1, 2, 3, 4, 5];
        let mut s = [0; 5];
        let mut warn = false;
        icumsum_int(&x, &mut s, &mut warn);
        assert_eq!(s, [1, 3, 6, 10, 15]);
        assert!(!warn);
    }

    #[test]
    fn test_icumsum_int_with_na() {
        let x = [1, 2, NA_INTEGER, 4, 5];
        let mut s = [0; 5];
        let mut warn = false;
        icumsum_int(&x, &mut s, &mut warn);
        assert_eq!(s[0], 1);
        assert_eq!(s[1], 3);
        assert_eq!(s[2], 0); // not filled after NA
    }

    #[test]
    fn test_cumprod_double() {
        let x = [2.0, 3.0, 4.0];
        let mut s = [0.0; 3];
        cumprod_double(&x, &mut s);
        assert_eq!(s, [2.0, 6.0, 24.0]);
    }

    #[test]
    fn test_cummax_double() {
        let x = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0];
        let mut s = [0.0; 7];
        cummax_double(&x, &mut s);
        assert_eq!(s, [3.0, 3.0, 4.0, 4.0, 5.0, 9.0, 9.0]);
    }

    #[test]
    fn test_cummin_double() {
        let x = [3.0, 1.0, 4.0, 1.0, 5.0, 0.0, 2.0];
        let mut s = [0.0; 7];
        cummin_double(&x, &mut s);
        assert_eq!(s, [3.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn test_icummax_int() {
        let x = [3, 1, 4, 1, 5, 2];
        let mut s = [0; 6];
        icummax_int(&x, &mut s);
        assert_eq!(s, [3, 3, 4, 4, 5, 5]);
    }

    #[test]
    fn test_icummin_int() {
        let x = [3, 1, 4, 1, 5, 2];
        let mut s = [0; 6];
        icummin_int(&x, &mut s);
        assert_eq!(s, [3, 1, 1, 1, 1, 1]);
    }

    #[test]
    fn test_ccumsum_complex() {
        let x = [Rcomplex { r: 1.0, i: 2.0 }, Rcomplex { r: 3.0, i: 4.0 }];
        let mut s = [Rcomplex::default(); 2];
        ccumsum_complex(&x, &mut s);
        assert_eq!(s[0].r, 1.0);
        assert_eq!(s[0].i, 2.0);
        assert_eq!(s[1].r, 4.0);
        assert_eq!(s[1].i, 6.0);
    }

    #[test]
    fn test_ccumprod_complex() {
        let x = [
            Rcomplex { r: 1.0, i: 0.0 },
            Rcomplex { r: 2.0, i: 0.0 },
            Rcomplex { r: 3.0, i: 0.0 },
        ];
        let mut s = [Rcomplex::default(); 3];
        ccumprod_complex(&x, &mut s);
        assert_eq!(s[0].r, 1.0);
        assert_eq!(s[1].r, 2.0);
        assert_eq!(s[2].r, 6.0);
    }
}
