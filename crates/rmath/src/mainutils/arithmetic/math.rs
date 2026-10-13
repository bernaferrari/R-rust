use super::*;

// ---------------------------------------------------------------------------
// SEXP-dependent implementations
// ---------------------------------------------------------------------------

/// Read the PRIMVAL (primitive offset) from a builtin/special SEXP.
#[inline]
pub(crate) unsafe fn primval(op: SEXP) -> c_int {
    unsafe { crate::mainutils::relop::PRIMVAL(op) }
}

/// Helper: check if SEXP is numeric (INTSXP, REALSXP, CPLXSXP, or LGLSXP).
#[inline]
pub(crate) unsafe fn is_numeric(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::LGLSXP
    }
}

/// Helper: check if SEXP is complex.
#[inline]
pub(crate) unsafe fn is_complex(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::CPLXSXP }
}

/// Helper: check if SEXP is integer or logical.
#[inline]
pub(crate) unsafe fn is_integer_or_logical(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP
    }
}

/// Helper: NO_REFERENCES check (NAMED == 0).
#[inline]
pub(crate) unsafe fn no_references(x: SEXP) -> bool {
    unsafe { NAMED(x) == 0 }
}

/// Integer-to-double conversion respecting NA_INTEGER.
#[inline]
pub(crate) fn r_integer_to_double(x: c_int) -> f64 {
    if x == NA_INTEGER { NA_REAL } else { x as f64 }
}

// ---- math1 helpers ----

/// Apply a unary math function f to each element of a REALSXP vector.
/// Preserves incoming NaN/NA. Issues warning on NaN produced from non-NaN input.
pub(crate) unsafe fn math1_impl(sa: SEXP, f: fn(f64) -> f64) -> SEXP {
    unsafe {
        if !is_numeric(sa) {
            return std::ptr::null_mut();
        }
        let n = XLENGTH(sa);
        // Coerce to REALSXP
        let sa = coerce_to_real(sa);
        let _sa_guard = protect(sa);

        let sy = if no_references(sa) {
            sa
        } else {
            Rf_allocVector3(SEXPTYPE::REALSXP, n)
        };
        let _sy_guard = protect(sy);

        let a = REAL(sa);
        let y = REAL(sy);
        let mut naflag = false;

        for i in 0..(n as usize) {
            let x = *a.add(i);
            *y.add(i) = f(x);
            if (*y.add(i)).is_nan() {
                if x.is_nan() {
                    *y.add(i) = x; // preserve incoming NaN
                } else {
                    naflag = true;
                }
            }
        }

        sy
    }
}

/// Apply a unary math function with special argument/result handling.
/// When x == arg, result is res. Otherwise applies f(x).
pub(crate) unsafe fn math1_ari_impl(sa: SEXP, f: fn(f64) -> f64, arg: f64, res: f64) -> SEXP {
    unsafe {
        if !is_numeric(sa) {
            return std::ptr::null_mut();
        }
        let n = XLENGTH(sa);
        let sa = coerce_to_real(sa);
        let _sa_guard = protect(sa);

        let sy = if no_references(sa) {
            sa
        } else {
            Rf_allocVector3(SEXPTYPE::REALSXP, n)
        };
        let _sy_guard = protect(sy);

        let a = REAL(sa);
        let y = REAL(sy);
        let mut naflag = false;

        for i in 0..(n as usize) {
            let x = *a.add(i);
            if x == arg {
                *y.add(i) = res;
            } else {
                *y.add(i) = f(x);
            }
            if (*y.add(i)).is_nan() {
                if x.is_nan() {
                    *y.add(i) = x;
                } else {
                    naflag = true;
                }
            }
        }

        sy
    }
}

/// Coerce a numeric SEXP to REALSXP (no-op if already REALSXP).
pub(crate) unsafe fn coerce_to_real(x: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::REALSXP {
            x
        } else if TYPEOF(x) == SEXPTYPE::INTSXP || TYPEOF(x) == SEXPTYPE::LGLSXP {
            let n = XLENGTH(x);
            let y = Rf_allocVector3(SEXPTYPE::REALSXP, n);
            let src = INTEGER(x);
            let dst = REAL(y);
            for i in 0..(n as usize) {
                let v = *src.add(i);
                *dst.add(i) = if v == NA_INTEGER { NA_REAL } else { v as f64 };
            }
            y
        } else {
            // CPLXSXP or other -- for now just return a zero-length REALSXP
            Rf_allocVector3(SEXPTYPE::REALSXP, 0)
        }
    }
}

/// Wrapper for extern "C" cospi to match fn(f64) -> f64 signature.
#[inline]
pub(crate) fn r_cospi(x: f64) -> f64 {
    cospi(x)
}

/// Wrapper for extern "C" sinpi to match fn(f64) -> f64 signature.
#[inline]
pub(crate) fn r_sinpi(x: f64) -> f64 {
    sinpi(x)
}

/// Wrapper for extern "C" tanpi to match fn(f64) -> f64 signature.
#[inline]
pub(crate) fn r_tanpi(x: f64) -> f64 {
    tanpi(x)
}

/// `sign` function for doubles (not in libm directly).
#[inline]
pub(crate) fn r_sign(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 {
        x // preserve NaN and signed zero
    } else if x > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// Complex math1: apply f to real and imaginary parts separately.
/// For functions like sqrt, log, exp on complex numbers.
pub(crate) unsafe fn complex_math1_impl(
    sa: SEXP,
    f_real: fn(f64) -> f64,
    f_imag: fn(f64) -> f64,
) -> SEXP {
    unsafe {
        let n = XLENGTH(sa);
        let sy = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
        let _sy_guard = protect(sy);
        let src = COMPLEX(sa);
        let dst = COMPLEX(sy);
        for i in 0..(n as usize) {
            let z = *src.add(i);
            *dst.add(i) = Rcomplex {
                r: f_real(z.r),
                i: f_imag(z.i),
            };
        }
        sy
    }
}

// ---- do_math1 ----

/// Single-argument math functions: sqrt, log, exp, floor, ceil, sign, etc.
///
/// Operation codes (from R's PRIMVAL):
///   1: floor, 2: ceil, 3: sqrt, 4: sign,
///   10: exp, 11: expm1, 12: log1p,
///   20: cos, 21: sin, 22: tan, 23: acos, 24: asin, 25: atan,
///   30: cosh, 31: sinh, 32: tanh, 33: acosh, 34: asinh, 35: atanh,
///   40: lgamma, 41: gamma, 42: digamma, 43: trigamma,
///   47: cospi, 48: sinpi, 49: tanpi
pub unsafe fn do_math1(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let code = primval(op);
        let sa = CAR(args);

        // Dispatch complex to complex handler
        if is_complex(sa) {
            return complex_math1_impl(sa, |x| x, |x| x);
        }

        match code {
            1 => math1_impl(sa, libm::floor),
            2 => math1_impl(sa, libm::ceil),
            3 => math1_impl(sa, Rsqrt),
            4 => math1_impl(sa, r_sign),
            10 => math1_ari_impl(sa, Rexp, 0.0, 1.0),
            11 => math1_ari_impl(sa, libm::expm1, 0.0, 0.0),
            12 => math1_ari_impl(sa, libm::log1p, 0.0, 0.0),
            20 => math1_ari_impl(sa, libm::cos, 0.0, 1.0),
            21 => math1_ari_impl(sa, Rsin, 0.0, 0.0),
            22 => math1_ari_impl(sa, Rtan, 0.0, 0.0),
            23 => math1_ari_impl(sa, libm::acos, 1.0, 0.0),
            24 => math1_ari_impl(sa, Rasin, 0.0, 0.0),
            25 => math1_ari_impl(sa, Ratan, 0.0, 0.0),
            30 => math1_ari_impl(sa, libm::cosh, 0.0, 1.0),
            31 => math1_ari_impl(sa, libm::sinh, 0.0, 0.0),
            32 => math1_ari_impl(sa, libm::tanh, 0.0, 0.0),
            33 => math1_ari_impl(sa, libm::acosh, 1.0, 0.0),
            34 => math1_ari_impl(sa, libm::asinh, 0.0, 0.0),
            35 => math1_ari_impl(sa, libm::atanh, 0.0, 0.0),
            40 => math1_impl(sa, lgammafn),
            41 => math1_impl(sa, gammafn),
            42 => math1_impl(sa, digamma),
            43 => math1_impl(sa, trigamma),
            47 => math1_impl(sa, r_cospi),
            48 => math1_impl(sa, r_sinpi),
            49 => math1_impl(sa, r_tanpi),
            _ => std::ptr::null_mut(),
        }
    }
}

// ---- math2 helpers ----

/// NA checking macro for two-argument math functions.
/// Mirrors R's if_NA_Math2_set.
#[inline]
pub(crate) fn na_math2_set(a: f64, b: f64) -> Option<f64> {
    // Check for R's NA (specific NaN bit pattern)
    let na_bits = crate::sexp::ffi::R_NA_BIT_PATTERN;
    let a_is_na = a.is_nan() && a.to_bits() == na_bits;
    let b_is_na = b.is_nan() && b.to_bits() == na_bits;
    if a_is_na || b_is_na {
        Some(crate::sexp::ffi::NA_REAL) // return R's NA_REAL
    } else if a.is_nan() || b.is_nan() {
        Some(f64::NAN)
    } else {
        None // neither NA nor NaN
    }
}

/// Apply a binary math function f(a, b) to element-wise pairs from two vectors
/// with recycling.
pub(crate) unsafe fn math2_impl(sa: SEXP, sb: SEXP, f: fn(f64, f64) -> f64) -> SEXP {
    unsafe {
        let na = XLENGTH(sa);
        let nb = XLENGTH(sb);

        // Zero-length handling
        if na == 0 || nb == 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }

        let n = if na > nb { na } else { nb };

        // Coerce both to REALSXP
        let sa = coerce_to_real(sa);
        let _sa_guard = protect(sa);
        let sb = coerce_to_real(sb);
        let _sb_guard = protect(sb);

        let sy = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _sy_guard = protect(sy);

        let a = REAL(sa);
        let b = REAL(sb);
        let y = REAL(sy);
        let mut naflag = false;

        for i in 0..(n as usize) {
            let ia = if na > 1 { i % (na as usize) } else { 0 };
            let ib = if nb > 1 { i % (nb as usize) } else { 0 };
            let ai = *a.add(ia);
            let bi = *b.add(ib);

            if let Some(val) = na_math2_set(ai, bi) {
                *y.add(i) = val;
            } else {
                *y.add(i) = f(ai, bi);
                if (*y.add(i)).is_nan() {
                    naflag = true;
                }
            }
        }

        sy
    }
}

// ---- do_math2 ----

/// Two-argument math functions: round, signif, atan2, etc.
///
/// Operation codes (from R's PRIMVAL):
///   0: atan2
///   10001: round (fround)
///   10004: signif (fprec)
pub unsafe fn do_math2(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let code = primval(op);

        match code {
            0 => {
                // atan2
                math2_impl(CAR(args), CADR(args), libm::atan2)
            }
            10001 => {
                // round
                math2_impl(CAR(args), CADR(args), fround)
            }
            10004 => {
                // signif
                math2_impl(CAR(args), CADR(args), fprec)
            }
            _ => std::ptr::null_mut(),
        }
    }
}
