//! Essentials domain module `mathstats` — extracted verbatim from essentials.rs.

use super::*;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;
use std::path::PathBuf;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
use crate::sexp::context::RError;
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// R's math2 builtins (2-arg math): log2, round, signif, trunc
// ---------------------------------------------------------------------------

/// R's `log2(x)` — log base 2 with optional explicit base override.
pub unsafe fn do_log2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let base_arg = CAR(CDR(args));
        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }
        let base = if base_arg.is_null() || base_arg == R_NilValue() {
            2.0
        } else {
            real_or_default(base_arg, std::f64::consts::E)
        };
        let n = XLENGTH(x_arg);
        let t = TYPEOF(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        let log_base = base.ln();
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x_arg).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v <= 0.0
            {
                NA_REAL
            } else {
                v.ln() / log_base
            };
        }
        result
    }
}

/// complex_math2's round/signif driver (r-source/src/main/complex.c):
/// coerce both operands to complex, recycle to max(na, nb), and apply `f`
/// part-wise with the digits scalar `b.r`. The all-NA rule yields
/// NA_complex_ only when every input part is NA; a NaN result part with no
/// NaN/NA input part raises stock's "NaNs produced in function \"%s\"".
unsafe fn math2_complex(
    call: SEXP,
    x_arg: SEXP,
    digits_arg: SEXP,
    dflt_digits: f64,
    name: &str,
    f: fn(re: f64, im: f64, digits: f64) -> (f64, f64),
) -> SEXP {
    unsafe {
        // Trunk coerces CAR(args) (x) to complex first and CADR(args)
        // (digits) second; x is already complex at this dispatch, so its
        // coercion is the identity. `sa` stays the value operand and `sb`
        // the digits operand, matching complex_math2's a/b roles.
        let sa = x_arg;
        let sb =
            if digits_arg.is_null() || digits_arg == R_NilValue() || digits_arg == R_MissingArg() {
                // do_Math2 fills the digits default upstream.
                let s = Rf_allocVector3(SEXPTYPE::CPLXSXP, 1);
                *COMPLEX(s) = Rcomplex {
                    r: dflt_digits,
                    i: 0.0,
                };
                s
            } else {
                crate::mainutils::coerce::coerceVector(digits_arg, SEXPTYPE::CPLXSXP.as_c_int())
            };
        let _sa_guard = protect(sa);
        let _sb_guard = protect(sb);

        let na = XLENGTH(sa);
        let nb = XLENGTH(sb);
        if na == 0 || nb == 0 {
            return Rf_allocVector3(SEXPTYPE::CPLXSXP, 0);
        }
        let n = if na < nb { nb } else { na };
        let sy = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
        let _sy_guard = protect(sy);

        let a = std::slice::from_raw_parts(COMPLEX(sa), na as usize);
        let b = std::slice::from_raw_parts(COMPLEX(sb), nb as usize);
        let y = std::slice::from_raw_parts_mut(COMPLEX(sy), n as usize);
        let na_bit = crate::sexp::ffi::R_NA_BIT_PATTERN;
        let mut naflag = false;
        // MOD_ITERATE2 recycling of x and digits.
        for i in 0..n as usize {
            let ai = a[i % na as usize];
            let bi = b[i % nb as usize];
            if ai.r.to_bits() == na_bit
                && ai.i.to_bits() == na_bit
                && bi.r.to_bits() == na_bit
                && bi.i.to_bits() == na_bit
            {
                y[i].r = NA_REAL;
                y[i].i = NA_REAL;
            } else {
                let (r, im) = f(ai.r, ai.i, bi.r);
                y[i].r = r;
                y[i].i = im;
                if (r.is_nan() || im.is_nan())
                    && !(ISNAN(ai.r) || ISNAN(ai.i) || ISNAN(bi.r) || ISNAN(bi.i))
                {
                    naflag = true;
                }
            }
        }
        if naflag {
            let msg =
                CString::new(format!("NaNs produced in function \"{name}\"")).unwrap_or_default();
            crate::mainutils::errors::Rf_warningcall1(call, msg.as_ptr());
        }
        if n == na {
            copy_all_attribs(sy, sa);
        } else if n == nb {
            copy_all_attribs(sy, sb);
        }
        sy
    }
}

/// Port of complex.c's `z_prec_r`: `signif()` for complex parts scales both
/// parts by the magnitude of the larger one (`m = max(|re|, |im|)`), so
/// they share a single exponent scale before `fround` applies.
fn z_prec_r(re: f64, im: f64, digits: f64) -> (f64, f64) {
    const MAX_DIGITS: i32 = 22; // complex.c's local MAX_DIGITS
    let m1 = re.abs();
    let m2 = im.abs();
    let mut m = 0.0f64;
    if m1.is_finite() {
        m = m1;
    }
    if m2.is_finite() && m2 > m {
        m = m2;
    }
    if m == 0.0 {
        return (re, im);
    }
    if !digits.is_finite() {
        if digits > 0.0 {
            return (re, im);
        }
        return (0.0, 0.0);
    }
    let mut dig = (digits + 0.5).floor() as i32;
    if dig > MAX_DIGITS {
        return (re, im);
    } else if dig < 1 {
        dig = 1;
    }
    let mag = m.log10().floor() as i32;
    dig = dig - mag - 1;
    if dig > 306 {
        let pow10 = 1.0e4f64;
        let digits = (dig - 4) as f64;
        (
            fround(pow10 * re, digits) / pow10,
            fround(pow10 * im, digits) / pow10,
        )
    } else {
        let digits = dig as f64;
        (fround(re, digits), fround(im, digits))
    }
}

/// SHALLOW_DUPLICATE_ATTRIB (attrib.h): shallow-copy every attribute of
/// `src` onto `dst`. The coerce-level port helper only carries a fixed set
/// of attributes; stock duplicates the whole list.
unsafe fn copy_all_attribs(dst: SEXP, src: SEXP) {
    unsafe {
        let mut attr = ATTRIB(src);
        while !attr.is_null() && attr != R_NilValue() {
            crate::eval::attrib_core::setAttrib(dst, TAG(attr), CAR(attr));
            attr = CDR(attr);
        }
    }
}

/// R's `round(x, digits=0)` — round to specified decimal digits.
pub unsafe fn do_round(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let digits_arg = CAR(CDR(args));
        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }
        // Stock routes complex x to complex_math2 (main/complex.c): round
        // each part with the same ties-even fround as the real path.
        if TYPEOF(x_arg) == SEXPTYPE::CPLXSXP {
            return math2_complex(call, x_arg, digits_arg, 0.0, "round", |re, im, digits| {
                (fround(re, digits), fround(im, digits))
            });
        }
        let digits = if digits_arg.is_null() || digits_arg == R_NilValue() {
            0.0
        } else {
            real_or_default(digits_arg, 0.0)
        };
        let n = XLENGTH(x_arg);
        let t = TYPEOF(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x_arg).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                NA_REAL
            } else {
                fround(v, digits)
            };
        }
        result
    }
}

/// Port of R's `fround` (r-source/src/nmath/fround.c): round `x` to `digits`
/// decimal digits with ties-to-even. Instead of a naive multiply-round-divide
/// (which double-rounds when `x * 10^dig` is itself inexact), it compares the
/// exact decimal candidates `floor(x*10^dig)/10^dig` and `ceil(x*10^dig)/10^dig`
/// and picks the nearer, breaking ties toward the even candidate.
fn fround(x: f64, digits: f64) -> f64 {
    const MAX_DIGITS: f64 = 323.0; // DBL_MAX_10_EXP + DBL_DIG
    const MAX10E: i32 = 308; // DBL_MAX_10_EXP
    const DBL_DIG: f64 = 15.0;

    if x.is_nan() || digits.is_nan() {
        return x + digits;
    }
    if !x.is_finite() {
        return x;
    }
    if digits > MAX_DIGITS || x == 0.0 {
        return x;
    }
    if digits < -(MAX10E as f64) {
        return 0.0;
    }
    if digits == 0.0 {
        return x.round_ties_even();
    }

    let dig = (digits + 0.5).floor() as i32;
    let sgn = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let l10x = std::f64::consts::LOG10_2 * (0.5 + logb(x));
    if l10x + dig as f64 > DBL_DIG {
        // Rounding to so many digits that no rounding is needed.
        return sgn * x;
    }
    let (pow10, p10): (f64, f64);
    let (xd, xu): (f64, f64);
    let i10: f64;
    if dig <= MAX10E {
        pow10 = r_pow_di(10.0, dig);
        p10 = 1.0;
    } else {
        p10 = r_pow_di(10.0, dig - MAX10E);
        pow10 = r_pow_di(10.0, MAX10E);
    }
    let x10 = if dig <= MAX10E {
        x * pow10
    } else {
        (x * pow10) * p10
    };
    i10 = x10.floor();
    if dig <= MAX10E {
        xd = i10 / pow10;
        xu = (i10 + 1.0) / pow10;
    } else {
        xd = i10 / pow10 / p10;
        xu = (i10 + 1.0) / pow10 / p10;
    }
    let du = xu - x;
    let dd = x - xd;
    sgn * (if du < dd || (i10 % 2.0 == 1.0 && du == dd) {
        xu
    } else {
        xd
    })
}

/// Port of R's `R_pow_di`: 10^n by binary exponentiation (matches C exactly).
fn r_pow_di(x: f64, n: i32) -> f64 {
    let mut n = n;
    let mut x = x;
    let mut dev = 1.0;
    if n == 0 {
        return 1.0;
    }
    if n < 0 {
        n = -n;
        x = 1.0 / x;
    }
    while n != 0 {
        if n & 1 != 0 {
            dev *= x;
        }
        x *= x;
        n >>= 1;
    }
    dev
}

/// logb(3): the integral binary exponent of `x` as a double.
fn logb(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return f64::NAN;
    }
    let bits = x.to_bits();
    let raw_exp = ((bits >> 52) & 0x7ff) as i32;
    if raw_exp == 0 {
        // subnormal: normalize
        let mut m = bits & 0x000f_ffff_ffff_ffff;
        let mut e = -1022;
        while m & 0x0010_0000_0000_0000 == 0 {
            m <<= 1;
            e -= 1;
        }
        e as f64
    } else {
        (raw_exp - 1023) as f64
    }
}

/// R's `signif(x, digits=6)` — round to significant digits.
///
/// Wires through the faithful `fprec` port (r-source/src/nmath/fprec.c),
/// mirroring how `do_round` routes through `fround`.
pub unsafe fn do_signif(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let digits_arg = CAR(CDR(args));
        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }
        // Stock routes complex x to complex_math2 (main/complex.c): apply
        // fprec (z_prec) to each part with the digits scalar.
        if TYPEOF(x_arg) == SEXPTYPE::CPLXSXP {
            return math2_complex(call, x_arg, digits_arg, 6.0, "signif", z_prec_r);
        }
        let digits = if digits_arg.is_null() || digits_arg == R_NilValue() {
            6.0
        } else {
            real_or_default(digits_arg, 6.0)
        };
        let n = XLENGTH(x_arg);
        let t = TYPEOF(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        // Mirror if_NA_Math2_set: NA in either operand yields NA (regular
        // NaN flows through fprec as x + digits, like upstream).
        let digits_is_na = digits.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN;
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x_arg).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            *dst.add(i as usize) =
                if digits_is_na || v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    NA_REAL
                } else {
                    crate::fprec::fprec(v, digits)
                };
        }
        result
    }
}

/// R's `trunc(x, ...)` — truncate toward zero with digits support.
pub unsafe fn do_trunc(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let _digits_arg = CAR(CDR(args));
        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x_arg);
        let t = TYPEOF(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x_arg).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                NA_REAL
            } else {
                v.trunc()
            };
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Math/Statistics
// ---------------------------------------------------------------------------

/// R's `cov(x, y)` — covariance between two numeric vectors.
pub unsafe fn do_cov(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y_cdr = CDR(args);
        let y = if y_cdr.is_null() || y_cdr == R_NilValue() {
            R_NilValue()
        } else {
            CAR(y_cdr)
        };

        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(NA_REAL);
        }

        let x_data = get_numeric_data(x);
        let y_data = if y.is_null() || y == R_NilValue() {
            x_data.clone()
        } else {
            get_numeric_data(y)
        };

        let n = x_data.len().min(y_data.len());
        if n == 0 {
            return Rf_ScalarReal(NA_REAL);
        }

        let mut sum_x = 0.0_f64;
        let mut sum_y = 0.0_f64;
        let mut count = 0_i64;
        for i in 0..n {
            if !x_data[i].is_nan() && !y_data[i].is_nan() {
                sum_x += x_data[i];
                sum_y += y_data[i];
                count += 1;
            }
        }
        if count < 2 {
            return Rf_ScalarReal(NA_REAL);
        }
        let mean_x = sum_x / count as f64;
        let mean_y = sum_y / count as f64;

        let mut cov = 0.0_f64;
        for i in 0..n {
            if !x_data[i].is_nan() && !y_data[i].is_nan() {
                cov += (x_data[i] - mean_x) * (y_data[i] - mean_y);
            }
        }
        Rf_ScalarReal(cov / (count as f64 - 1.0))
    }
}

/// R's `cor(x, y)` — Pearson correlation between two numeric vectors.
pub unsafe fn do_cor(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y_cdr = CDR(args);
        let y = if y_cdr.is_null() || y_cdr == R_NilValue() || CAR(y_cdr) == R_MissingArg() {
            R_NilValue()
        } else {
            CAR(y_cdr)
        };

        if x.is_null() || x == R_NilValue() || x == R_MissingArg() {
            return Rf_ScalarReal(NA_REAL);
        }

        let mut extra = if y_cdr.is_null() || y_cdr == R_NilValue() {
            R_NilValue()
        } else {
            CDR(y_cdr)
        };
        let mut position = 2;
        let mut use_mode = "everything".to_string();
        let mut method_mode = "pearson".to_string();
        while !extra.is_null() && extra != R_NilValue() {
            let name = tag_name(extra).unwrap_or_else(|| {
                if position == 2 {
                    "use".into()
                } else if position == 3 {
                    "method".into()
                } else {
                    "unknown".into()
                }
            });
            let value = CAR(extra);
            if name == "method" {
                if value != R_MissingArg()
                    && (TYPEOF(value) != SEXPTYPE::STRSXP || XLENGTH(value) != 1 || {
                        let method =
                            std::ffi::CStr::from_ptr(CHAR(STRING_ELT(value, 0))).to_bytes();
                        method != b"pearson" && method != b"spearman"
                    })
                {
                    base_error("unsupported cor method");
                }
                if value != R_MissingArg() {
                    method_mode = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(value, 0)))
                        .to_string_lossy()
                        .into_owned();
                }
            } else if name == "use" {
                if value != R_MissingArg() {
                    if TYPEOF(value) != SEXPTYPE::STRSXP || XLENGTH(value) != 1 {
                        base_error("invalid 'use' argument");
                    }
                    use_mode = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(value, 0)))
                        .to_string_lossy()
                        .into_owned();
                    if !matches!(
                        use_mode.as_str(),
                        "everything"
                            | "all.obs"
                            | "complete.obs"
                            | "na.or.complete"
                            | "pairwise.complete.obs"
                    ) {
                        base_error("invalid 'use' argument");
                    }
                }
            } else {
                base_error("unsupported cor argument");
            }
            position += 1;
            extra = CDR(extra);
        }

        // Matrices are column-major in R.  Support Pearson correlation for
        // finite numeric columns while retaining the scalar vector path below.
        let dims = |value: SEXP| -> Option<(usize, usize)> {
            let dim =
                crate::eval::attrib_core::getAttrib(value, crate::eval::attrib_core::R_DimSymbol());
            if TYPEOF(dim) != SEXPTYPE::INTSXP || XLENGTH(dim) != 2 {
                return None;
            }
            let rows = *INTEGER(dim) as isize;
            let cols = *INTEGER(dim).add(1) as isize;
            (rows >= 0 && cols >= 0).then_some((rows as usize, cols as usize))
        };
        let x_dims = dims(x);
        let y_missing = y.is_null() || y == R_NilValue() || y == R_MissingArg();
        let y_dims = if y.is_null() || y == R_NilValue() || y == R_MissingArg() {
            None
        } else {
            dims(y)
        };
        if x_dims.is_some() || y_dims.is_some() {
            if use_mode != "everything" || method_mode != "pearson" {
                base_error("matrix cor currently supports only use='everything', method='pearson'");
            }
            let (x_rows, nx) = x_dims.unwrap_or((XLENGTH(x) as usize, 1));
            let (y_rows, ny) = match y_dims {
                Some((rows, cols)) => (rows, cols),
                None if x_dims.is_some() && y_missing => (x_rows, nx),
                None => (x_rows, 1),
            };
            if y_rows != x_rows {
                base_error(format!("incompatible dimensions ({x_rows} vs {y_rows})"));
            }
            let x_data = get_numeric_data(x);
            let y_data = if y.is_null() || y == R_NilValue() || y == R_MissingArg() {
                x_data.clone()
            } else {
                get_numeric_data(y)
            };
            // The matrix implementation intentionally covers the finite,
            // default Pearson case only.  Do not silently turn R's default
            // `use = "everything"` NA semantics into pairwise deletion.
            if x_data.iter().any(|value| !value.is_finite())
                || y_data.iter().any(|value| !value.is_finite())
            {
                base_error("matrix cor currently requires finite numeric data");
            }
            let x_is_matrix = x_dims.is_some();
            let y_is_matrix = y_dims.is_some() || y_missing && x_is_matrix;
            if (!x_is_matrix && x_data.len() != x_rows) || (!y_is_matrix && y_data.len() != y_rows)
            {
                base_error("incompatible dimensions");
            }
            if x_rows.checked_mul(nx) != Some(x_data.len())
                || y_rows.checked_mul(ny) != Some(y_data.len())
            {
                base_error("matrix dimensions do not match correlation data");
            }
            let result_len = nx
                .checked_mul(ny)
                .unwrap_or_else(|| base_error("correlation matrix is too large"));
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, result_len as i64);
            let _guard = protect(result);
            for j in 0..ny {
                for i in 0..nx {
                    let (mut sx, mut sy, mut count) = (0., 0., 0usize);
                    for r in 0..x_rows {
                        let xv = x_data[if x_is_matrix { i * x_rows + r } else { r }];
                        let yv = y_data[if y_is_matrix { j * y_rows + r } else { r }];
                        if xv.is_finite() && yv.is_finite() {
                            sx += xv;
                            sy += yv;
                            count += 1;
                        }
                    }
                    let value = if count < 2 {
                        NA_REAL
                    } else {
                        let (mx, my) = (sx / count as f64, sy / count as f64);
                        let (mut cov, mut vx, mut vy) = (0., 0., 0.);
                        for r in 0..x_rows {
                            let xv = x_data[if x_is_matrix { i * x_rows + r } else { r }];
                            let yv = y_data[if y_is_matrix { j * y_rows + r } else { r }];
                            if xv.is_finite() && yv.is_finite() {
                                let dx = xv - mx;
                                let dy = yv - my;
                                cov += dx * dy;
                                vx += dx * dx;
                                vy += dy * dy;
                            }
                        }
                        let denom = (vx * vy).sqrt();
                        if denom == 0. { NA_REAL } else { cov / denom }
                    };
                    *REAL(result).add(j * nx + i) = value;
                }
            }
            // R returns a matrix whenever either argument is a matrix,
            // including matrix/vector and vector/matrix correlations.
            if x_is_matrix || y_is_matrix {
                let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
                let _dim_guard = protect(dim);
                *INTEGER(dim) = nx as i32;
                *INTEGER(dim).add(1) = ny as i32;
                crate::eval::attrib_core::setAttrib(
                    result,
                    crate::eval::attrib_core::R_DimSymbol(),
                    dim,
                );
                let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
                let _dng = protect(dimnames);
                let column_names = |value: SEXP| {
                    let dn = crate::eval::attrib_core::getAttrib(
                        value,
                        crate::eval::attrib_core::R_DimNamesSymbol(),
                    );
                    if TYPEOF(dn) == SEXPTYPE::VECSXP && XLENGTH(dn) >= 2 {
                        VECTOR_ELT(dn, 1)
                    } else {
                        R_NilValue()
                    }
                };
                SET_VECTOR_ELT(
                    dimnames,
                    0,
                    if x_is_matrix {
                        column_names(x)
                    } else {
                        R_NilValue()
                    },
                );
                SET_VECTOR_ELT(
                    dimnames,
                    1,
                    if y_is_matrix {
                        if y.is_null() || y == R_NilValue() || y == R_MissingArg() {
                            column_names(x)
                        } else {
                            column_names(y)
                        }
                    } else if y.is_null() || y == R_NilValue() || y == R_MissingArg() {
                        column_names(x)
                    } else {
                        R_NilValue()
                    },
                );
                crate::eval::attrib_core::setAttrib(
                    result,
                    crate::eval::attrib_core::R_DimNamesSymbol(),
                    dimnames,
                );
            }
            return result;
        }

        let x_data = get_numeric_data(x);
        let y_data = if y.is_null() || y == R_NilValue() || y == R_MissingArg() {
            x_data.clone()
        } else {
            get_numeric_data(y)
        };

        if x_data.len() != y_data.len() {
            base_error("incompatible lengths");
        }
        let n = x_data.len();
        if n == 0 {
            match use_mode.as_str() {
                "complete.obs" => base_error("no complete element pairs"),
                "pairwise.complete.obs" => base_error("'x' is empty"),
                _ => return Rf_ScalarReal(NA_REAL),
            }
        }

        let missing = |value: f64| value.is_nan();
        let missing_count = (0..n)
            .filter(|&i| missing(x_data[i]) || missing(y_data[i]))
            .count();
        match use_mode.as_str() {
            "everything" if missing_count > 0 => return Rf_ScalarReal(NA_REAL),
            "all.obs" if missing_count > 0 => base_error("missing observations in cov/cor"),
            _ => {}
        }

        let ranked: Option<(Vec<f64>, Vec<f64>)> = if method_mode == "spearman" {
            let active: Vec<usize> = (0..n)
                .filter(|&i| {
                    use_mode == "everything" || (!missing(x_data[i]) && !missing(y_data[i]))
                })
                .collect();
            let rank = |values: &[f64]| -> Vec<f64> {
                let mut order: Vec<usize> = (0..values.len()).collect();
                order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
                let mut ranks = vec![0.0; values.len()];
                let mut start = 0;
                while start < order.len() {
                    let mut end = start + 1;
                    while end < order.len() && values[order[end]] == values[order[start]] {
                        end += 1;
                    }
                    let average = (start + end + 1) as f64 / 2.0;
                    for &index in &order[start..end] {
                        ranks[index] = average;
                    }
                    start = end;
                }
                ranks
            };
            let xv: Vec<f64> = active.iter().map(|&i| x_data[i]).collect();
            let yv: Vec<f64> = active.iter().map(|&i| y_data[i]).collect();
            Some((rank(&xv), rank(&yv)))
        } else {
            None
        };

        let mut sum_x = 0.0_f64;
        let mut sum_y = 0.0_f64;
        let count = if let Some((calc_x, calc_y)) = ranked.as_ref() {
            for i in 0..calc_x.len() {
                sum_x += calc_x[i];
                sum_y += calc_y[i];
            }
            calc_x.len() as i64
        } else {
            let mut count = 0_i64;
            for i in 0..n {
                if use_mode == "everything" || (!missing(x_data[i]) && !missing(y_data[i])) {
                    sum_x += x_data[i];
                    sum_y += y_data[i];
                    count += 1;
                }
            }
            count
        };
        if count < 2 {
            if count == 0 && use_mode == "complete.obs" {
                base_error("no complete element pairs");
            }
            return Rf_ScalarReal(NA_REAL);
        }
        let mean_x = sum_x / count as f64;
        let mean_y = sum_y / count as f64;

        let mut cov = 0.0_f64;
        let mut var_x = 0.0_f64;
        let mut var_y = 0.0_f64;
        if let Some((calc_x, calc_y)) = ranked.as_ref() {
            for i in 0..calc_x.len() {
                let dx = calc_x[i] - mean_x;
                let dy = calc_y[i] - mean_y;
                cov += dx * dy;
                var_x += dx * dx;
                var_y += dy * dy;
            }
        } else {
            for i in 0..n {
                if use_mode == "everything" || (!missing(x_data[i]) && !missing(y_data[i])) {
                    let dx = x_data[i] - mean_x;
                    let dy = y_data[i] - mean_y;
                    cov += dx * dy;
                    var_x += dx * dx;
                    var_y += dy * dy;
                }
            }
        }
        let denom = (var_x * var_y).sqrt();
        if denom == 0.0 {
            crate::mainutils::errors::Rf_warningcall1(
                call,
                c"the standard deviation is zero".as_ptr(),
            );
            return Rf_ScalarReal(NA_REAL);
        }
        Rf_ScalarReal(cov / denom)
    }
}

/// R's `scale(x, center=TRUE, scale=TRUE)` — standardize a numeric vector.
pub unsafe fn do_scale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let center_arg = CAR(CDR(args));
        let scale_arg = CAR(CDR(CDR(args)));

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let do_center = center_arg.is_null()
            || center_arg == R_NilValue()
            || (TYPEOF(center_arg) == SEXPTYPE::LGLSXP && *LOGICAL(center_arg) == TRUE);
        let do_scale = scale_arg.is_null()
            || scale_arg == R_NilValue()
            || (TYPEOF(scale_arg) == SEXPTYPE::LGLSXP && *LOGICAL(scale_arg) == TRUE);

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        // Compute mean
        let mut sum = 0.0_f64;
        let mut count = 0_i64;
        for i in 0..n {
            let v = real_or_default(elt_to_sexp(x, i), NA_REAL);
            if !v.is_nan() && v != NA_REAL {
                sum += v;
                count += 1;
            }
        }
        let mean = if count > 0 {
            sum / count as f64
        } else {
            NA_REAL
        };

        // Compute sd
        let mut var_sum = 0.0_f64;
        if do_scale {
            for i in 0..n {
                let v = real_or_default(elt_to_sexp(x, i), NA_REAL);
                if !v.is_nan() && v != NA_REAL {
                    var_sum += (v - mean) * (v - mean);
                }
            }
        }
        let sd = if count > 1 {
            (var_sum / (count as f64 - 1.0)).sqrt()
        } else {
            NA_REAL
        };

        let dst = REAL(result);
        for i in 0..n {
            let v = real_or_default(elt_to_sexp(x, i), NA_REAL);
            let centered = if do_center { v - mean } else { v };
            let scaled = if do_scale && sd != 0.0 && !sd.is_nan() {
                centered / sd
            } else {
                centered
            };
            *dst.add(i as usize) = scaled;
        }
        result
    }
}

/// R's `rle(x)` — run-length encoding.
pub unsafe fn do_rle(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let has_dim = !dim.is_null() && dim != R_NilValue();
        let atomic = matches!(
            TYPEOF(x),
            t if t == SEXPTYPE::LGLSXP
                || t == SEXPTYPE::INTSXP
                || t == SEXPTYPE::REALSXP
                || t == SEXPTYPE::CPLXSXP
                || t == SEXPTYPE::STRSXP
                || t == SEXPTYPE::RAWSXP
        );
        if has_dim || !(atomic || TYPEOF(x) == SEXPTYPE::VECSXP) {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'x' must be a vector of an atomic type",
            );
        }

        let n = XLENGTH(x);
        if n == 0 {
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            SET_VECTOR_ELT(result, 0, Rf_allocVector3(SEXPTYPE::INTSXP, 0));
            SET_VECTOR_ELT(result, 1, Rf_allocVector3(TYPEOF(x), 0));
            set_rle_attrs(result);
            return result;
        }

        // Collect run lengths and starting indices. Missing values are never
        // equal to the previous value in GNU R's rle().
        let mut lengths: Vec<i32> = Vec::new();
        let mut value_indices: Vec<R_xlen_t> = Vec::new();

        value_indices.push(0);
        lengths.push(1);

        for i in 1..n {
            let last_start = *value_indices.last().unwrap_or(&0);
            if rle_values_equal(x, i, last_start) {
                let last_idx = lengths.len() - 1;
                lengths[last_idx] += 1;
            } else {
                value_indices.push(i);
                lengths.push(1);
            }
        }

        let n_runs = lengths.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let lengths_sexp = Rf_allocVector3(SEXPTYPE::INTSXP, n_runs);
        let values_sexp = Rf_allocVector3(TYPEOF(x), n_runs);
        let _p2 = protect(lengths_sexp);
        let _p3 = protect(values_sexp);

        let dst_l = INTEGER(lengths_sexp);
        for i in 0..n_runs {
            *dst_l.add(i as usize) = lengths[i as usize];
            copy_vector_elt(values_sexp, i, x, value_indices[i as usize]);
        }

        SET_VECTOR_ELT(result, 0, lengths_sexp);
        SET_VECTOR_ELT(result, 1, values_sexp);
        set_rle_attrs(result);
        result
    }
}

unsafe fn set_rle_attrs(x: SEXP) {
    unsafe {
        set_string_names(x, &["lengths".to_string(), "values".to_string()]);
        let class = Rf_mkString(c"rle".as_ptr());
        if !class.is_null() {
            let _class_guard = protect(class);
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }
    }
}

unsafe fn rle_values_equal(x: SEXP, lhs: R_xlen_t, rhs: R_xlen_t) -> bool {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP => {
                let a = *INTEGER(x).add(lhs as usize);
                let b = *INTEGER(x).add(rhs as usize);
                a != NA_INTEGER && b != NA_INTEGER && a == b
            }
            t if t == SEXPTYPE::REALSXP => {
                let a = *REAL(x).add(lhs as usize);
                let b = *REAL(x).add(rhs as usize);
                !ISNAN(a) && !ISNAN(b) && a == b
            }
            t if t == SEXPTYPE::STRSXP => {
                let a = STRING_ELT(x, lhs);
                let b = STRING_ELT(x, rhs);
                a != crate::sexp::globals::R_NaString()
                    && b != crate::sexp::globals::R_NaString()
                    && elt_to_string(x, lhs) == elt_to_string(x, rhs)
            }
            t if t == SEXPTYPE::RAWSXP => *RAW(x).add(lhs as usize) == *RAW(x).add(rhs as usize),
            _ => false,
        }
    }
}

/// R's `inverse.rle(x)` — inverse of run-length encoding.
pub unsafe fn do_inverse_rle(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }

        let lengths_sexp = VECTOR_ELT(x, 0);
        let values_sexp = VECTOR_ELT(x, 1);
        if lengths_sexp.is_null() || values_sexp.is_null() {
            return R_NilValue();
        }

        let n_runs = XLENGTH(lengths_sexp);
        if n_runs == 0 {
            return Rf_allocVector3(TYPEOF(values_sexp), 0);
        }

        // Compute total length
        let mut total: R_xlen_t = 0;
        for i in 0..n_runs {
            total += (*INTEGER(lengths_sexp).add(i as usize)) as R_xlen_t;
        }

        let result = Rf_allocVector3(TYPEOF(values_sexp), total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let mut offset: R_xlen_t = 0;
        for i in 0..n_runs {
            let len = *INTEGER(lengths_sexp).add(i as usize);
            for j in 0..len {
                copy_vector_elt(result, offset + j as R_xlen_t, values_sexp, i);
            }
            offset += len as R_xlen_t;
        }
        result
    }
}

/// GNU `rowsum(x, group)` — sum rows by group.
pub unsafe fn do_rowsum(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let group = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let xt = TYPEOF(x);
        if xt != SEXPTYPE::INTSXP && xt != SEXPTYPE::REALSXP && xt != SEXPTYPE::LGLSXP {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'x' must be numeric",
            );
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (nr, nc) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as i64, *INTEGER(dim).add(1) as i64)
        } else {
            (XLENGTH(x), 1)
        };
        if XLENGTH(group) != nr {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "incorrect length for 'group'",
            );
        }
        let mut labels: Vec<String> = Vec::new();
        let mut index: Vec<usize> = Vec::new();
        for i in 0..nr {
            let key = format_rowsum_group(group, i);
            if let Some(pos) = labels.iter().position(|s| s == &key) {
                index.push(pos);
            } else {
                index.push(labels.len());
                labels.push(key);
            }
        }
        let mut order: Vec<usize> = (0..labels.len()).collect();
        order.sort_by(|&a, &b| labels[a].cmp(&labels[b]));
        let mut new_pos = vec![0usize; labels.len()];
        let mut sorted_labels = vec![String::new(); labels.len()];
        for (dst, &src) in order.iter().enumerate() {
            new_pos[src] = dst;
            sorted_labels[dst].clone_from(&labels[src]);
        }
        let ng = labels.len() as i64;
        let out_ty = if xt == SEXPTYPE::REALSXP {
            SEXPTYPE::REALSXP
        } else {
            SEXPTYPE::INTSXP
        };
        let result = Rf_allocVector3(out_ty, ng * nc);
        let _result = protect(result);
        if out_ty == SEXPTYPE::REALSXP {
            for i in 0..(ng * nc) as usize {
                *REAL(result).add(i) = 0.0;
            }
        } else {
            for i in 0..(ng * nc) as usize {
                *INTEGER(result).add(i) = 0;
            }
        }
        for col in 0..nc {
            for row in 0..nr {
                let g = new_pos[index[row as usize]] as i64;
                let src = row + col * nr;
                let dst = g + col * ng;
                let val = if xt == SEXPTYPE::REALSXP {
                    *REAL(x).add(src as usize)
                } else {
                    *INTEGER(x).add(src as usize) as f64
                };
                if out_ty == SEXPTYPE::REALSXP {
                    *REAL(result).add(dst as usize) += val;
                } else {
                    *INTEGER(result).add(dst as usize) += val as i32;
                }
            }
        }
        let out_dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(out_dim) = ng as c_int;
        *INTEGER(out_dim).add(1) = nc as c_int;
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimSymbol(),
            out_dim,
        );
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, ng);
        for (i, lab) in sorted_labels.iter().enumerate() {
            let cstr = CString::new(lab.as_str()).unwrap_or_default();
            SET_STRING_ELT(rn, i as i64, Rf_mkChar(cstr.as_ptr()));
        }
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, R_NilValue());
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dn,
        );
        result
    }
}

unsafe fn format_rowsum_group(group: SEXP, i: i64) -> String {
    unsafe {
        match TYPEOF(group) {
            t if t == SEXPTYPE::STRSXP => {
                let s = STRING_ELT(group, i);
                if s.is_null() {
                    "NA".into()
                } else {
                    std::ffi::CStr::from_ptr(CHAR(s))
                        .to_string_lossy()
                        .into_owned()
                }
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                format!("{}", *INTEGER(group).add(i as usize))
            }
            t if t == SEXPTYPE::REALSXP => format!("{}", *REAL(group).add(i as usize)),
            _ => i.to_string(),
        }
    }
}

/// GNU `jitter(x, factor=1, amount=NULL)`.
pub unsafe fn do_jitter(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || XLENGTH(x) == 0 {
            return x;
        }
        let xt = TYPEOF(x);
        if xt != SEXPTYPE::INTSXP && xt != SEXPTYPE::REALSXP && xt != SEXPTYPE::LGLSXP {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'x' must be numeric",
            );
        }
        let n = XLENGTH(x);
        let mut finite: Vec<f64> = Vec::new();
        for i in 0..n {
            let v = if xt == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                *INTEGER(x).add(i as usize) as f64
            };
            if v.is_finite() {
                finite.push(v);
            }
        }
        let (lo, hi) = finite
            .iter()
            .copied()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                (a.min(v), b.max(v))
            });
        let mut z = hi - lo;
        if z == 0.0 {
            z = lo.abs();
        }
        if z == 0.0 {
            z = 1.0;
        }
        let mut factor = 1.0;
        let mut amount_arg = R_NilValue();
        let mut cell = CDR(args);
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let slot = if name == "factor" {
                0
            } else if name == "amount" {
                1
            } else {
                let s = pos;
                pos += 1;
                s
            };
            if slot == 0 {
                factor = if TYPEOF(CAR(cell)) == SEXPTYPE::REALSXP {
                    *REAL(CAR(cell))
                } else if TYPEOF(CAR(cell)) == SEXPTYPE::INTSXP {
                    *INTEGER(CAR(cell)) as f64
                } else {
                    1.0
                };
            } else if slot == 1 {
                amount_arg = CAR(cell);
            }
            cell = CDR(cell);
        }
        let amount = if amount_arg.is_null() || amount_arg == R_NilValue() {
            let digits = 3 - (z.log10().floor() as i32);
            let p = 10f64.powi(digits);
            let mut rounded: Vec<f64> = finite.iter().map(|v| (v * p).round() / p).collect();
            rounded.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            rounded.dedup();
            let d = if rounded.len() >= 2 {
                rounded
                    .windows(2)
                    .map(|w| w[1] - w[0])
                    .fold(f64::INFINITY, f64::min)
            } else if rounded.first().copied().unwrap_or(0.0) != 0.0 {
                rounded[0] / 10.0
            } else {
                z / 10.0
            };
            factor / 5.0 * d.abs()
        } else {
            let a = if TYPEOF(amount_arg) == SEXPTYPE::REALSXP {
                *REAL(amount_arg)
            } else if TYPEOF(amount_arg) == SEXPTYPE::INTSXP {
                *INTEGER(amount_arg) as f64
            } else {
                0.0
            };
            if a == 0.0 { factor * (z / 50.0) } else { a }
        };
        let n_s = Rf_ScalarInteger(n as c_int);
        let _n = protect(n_s);
        let a_s = Rf_ScalarReal(-1.0);
        let _a = protect(a_s);
        let b_s = Rf_ScalarReal(1.0);
        let _b = protect(b_s);
        let u = crate::library::stats::random::do_runif(n_s, a_s, b_s);
        let _u = protect(u);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let xv = if xt == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                *INTEGER(x).add(i as usize) as f64
            };
            let uv = if TYPEOF(u) == SEXPTYPE::REALSXP {
                *REAL(u).add(i as usize)
            } else {
                0.0
            };
            *REAL(result).add(i as usize) = xv + amount * uv;
        }
        result
    }
}

/// GNU `margin.table(x, margin)`.
pub unsafe fn do_margin_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let margin_arg = CAR(CDR(args));
        let xt = TYPEOF(x);
        if xt != SEXPTYPE::INTSXP && xt != SEXPTYPE::REALSXP && xt != SEXPTYPE::LGLSXP {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'x' is not an array",
            );
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (nr, nc) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as i64, *INTEGER(dim).add(1) as i64)
        } else {
            (XLENGTH(x), 1)
        };
        let has_margin =
            !margin_arg.is_null() && margin_arg != R_NilValue() && XLENGTH(margin_arg) > 0;
        if !has_margin {
            let mut acc = 0.0;
            let n = XLENGTH(x);
            for i in 0..n {
                acc += if xt == SEXPTYPE::REALSXP {
                    *REAL(x).add(i as usize)
                } else {
                    *INTEGER(x).add(i as usize) as f64
                };
            }
            return if xt == SEXPTYPE::REALSXP {
                Rf_ScalarReal(acc)
            } else {
                Rf_ScalarInteger(acc as c_int)
            };
        }
        let margin = if TYPEOF(margin_arg) == SEXPTYPE::INTSXP {
            *INTEGER(margin_arg)
        } else if TYPEOF(margin_arg) == SEXPTYPE::REALSXP {
            *REAL(margin_arg) as c_int
        } else {
            1
        };
        let out_n = if margin == 1 { nr } else { nc };
        let result = Rf_allocVector3(
            if xt == SEXPTYPE::REALSXP {
                SEXPTYPE::REALSXP
            } else {
                SEXPTYPE::INTSXP
            },
            out_n,
        );
        let _r = protect(result);
        if xt == SEXPTYPE::REALSXP {
            for i in 0..out_n as usize {
                *REAL(result).add(i) = 0.0;
            }
        } else {
            for i in 0..out_n as usize {
                *INTEGER(result).add(i) = 0;
            }
        }
        for col in 0..nc {
            for row in 0..nr {
                let src = row + col * nr;
                let val = if xt == SEXPTYPE::REALSXP {
                    *REAL(x).add(src as usize)
                } else {
                    *INTEGER(x).add(src as usize) as f64
                };
                let dst = if margin == 1 { row } else { col };
                if xt == SEXPTYPE::REALSXP {
                    *REAL(result).add(dst as usize) += val;
                } else {
                    *INTEGER(result).add(dst as usize) += val as i32;
                }
            }
        }
        let out_dim = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
        *INTEGER(out_dim) = out_n as c_int;
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimSymbol(),
            out_dim,
        );
        result
    }
}

/// GNU `mad(x)` — median absolute deviation times 1.4826.
pub unsafe fn do_mad(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(NA_REAL);
        }
        let xt = TYPEOF(x);
        if xt != SEXPTYPE::INTSXP && xt != SEXPTYPE::REALSXP && xt != SEXPTYPE::LGLSXP {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'x' must be numeric",
            );
        }
        let mut constant = 1.4826;
        let mut cell = CDR(args);
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "constant" || (name.is_empty() && pos == 1) {
                let v = CAR(cell);
                if TYPEOF(v) == SEXPTYPE::REALSXP {
                    constant = *REAL(v);
                } else if TYPEOF(v) == SEXPTYPE::INTSXP {
                    constant = *INTEGER(v) as f64;
                }
            }
            if name.is_empty() {
                pos += 1;
            }
            cell = CDR(cell);
        }
        let med_args = Rf_cons(x, R_NilValue());
        let _ma = protect(med_args);
        let center = crate::mainutils::essentials::do_median(call, op, med_args, rho);
        let _c = protect(center);
        let cval = if TYPEOF(center) == SEXPTYPE::REALSXP {
            *REAL(center)
        } else if TYPEOF(center) == SEXPTYPE::INTSXP {
            *INTEGER(center) as f64
        } else {
            NA_REAL
        };
        let n = XLENGTH(x);
        let absv = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _a = protect(absv);
        for i in 0..n {
            let xv = if xt == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                *INTEGER(x).add(i as usize) as f64
            };
            *REAL(absv).add(i as usize) = (xv - cval).abs();
        }
        let abs_args = Rf_cons(absv, R_NilValue());
        let _aa = protect(abs_args);
        let med = crate::mainutils::essentials::do_median(call, op, abs_args, rho);
        let mval = if TYPEOF(med) == SEXPTYPE::REALSXP {
            *REAL(med)
        } else {
            0.0
        };
        Rf_ScalarReal(constant * mval)
    }
}

/// GNU `fivenum(x)` — Tukey five-number summary.
pub unsafe fn do_fivenum(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, 5);
            for i in 0..5 {
                *REAL(result).add(i) = NA_REAL;
            }
            return result;
        }
        let xt = TYPEOF(x);
        let n0 = XLENGTH(x);
        let mut vals: Vec<f64> = Vec::new();
        for i in 0..n0 {
            let v = if xt == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if xt == SEXPTYPE::INTSXP || xt == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            if v.is_finite() {
                vals.push(v);
            }
        }
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = vals.len();
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 5);
        if n == 0 {
            for i in 0..5 {
                *REAL(result).add(i) = NA_REAL;
            }
            return result;
        }
        let n4 = (((n + 3) / 2) as f64) / 2.0;
        let d = [
            1.0,
            n4,
            (n as f64 + 1.0) / 2.0,
            n as f64 + 1.0 - n4,
            n as f64,
        ];
        for (i, di) in d.iter().enumerate() {
            let lo = di.floor() as usize;
            let hi = di.ceil() as usize;
            let a = vals[lo.saturating_sub(1).min(n - 1)];
            let b = vals[hi.saturating_sub(1).min(n - 1)];
            *REAL(result).add(i) = 0.5 * (a + b);
        }
        result
    }
}

/// GNU `boxplot.stats(x, coef=1.5)`.
pub unsafe fn do_boxplot_stats(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mut coef = 1.5;
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let c = CAR(rest);
            if !c.is_null() && c != R_NilValue() {
                coef = elt_real_safe(c, 0);
            }
        }
        let stats = do_fivenum(_call, _op, Rf_cons(x, R_NilValue()), _rho);
        let _s = protect(stats);
        let q1 = *REAL(stats).add(1);
        let med = *REAL(stats).add(2);
        let q3 = *REAL(stats).add(3);
        let iqr = q3 - q1;
        let n0 = if x.is_null() || x == R_NilValue() {
            0
        } else {
            XLENGTH(x)
        };
        let xt = if x.is_null() || x == R_NilValue() {
            SEXPTYPE::REALSXP
        } else {
            SEXPTYPE(TYPEOF(x))
        };
        let mut n = 0i32;
        let mut outs: Vec<f64> = Vec::new();
        let mut insides: Vec<f64> = Vec::new();
        for i in 0..n0 {
            let v = if xt == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if xt == SEXPTYPE::INTSXP || xt == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v.is_nan() {
                continue;
            }
            n += 1;
            let is_out =
                coef > 0.0 && iqr.is_finite() && (v < q1 - coef * iqr || v > q3 + coef * iqr);
            if is_out {
                outs.push(v);
            } else if v.is_finite() {
                insides.push(v);
            }
        }
        if !insides.is_empty() && coef > 0.0 {
            let lo = insides.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = insides.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            *REAL(stats) = lo;
            *REAL(stats).add(4) = hi;
        }
        let conf = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _c = protect(conf);
        if n > 0 && iqr.is_finite() {
            let half = 1.58 * iqr / (n as f64).sqrt();
            *REAL(conf) = med - half;
            *REAL(conf).add(1) = med + half;
        } else {
            *REAL(conf) = NA_REAL;
            *REAL(conf).add(1) = NA_REAL;
        }
        let out = Rf_allocVector3(SEXPTYPE::REALSXP, outs.len() as i64);
        let _o = protect(out);
        for (i, v) in outs.iter().enumerate() {
            *REAL(out).add(i) = *v;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, stats);
        SET_VECTOR_ELT(result, 1, Rf_ScalarInteger(n));
        SET_VECTOR_ELT(result, 2, conf);
        SET_VECTOR_ELT(result, 3, out);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "stats".to_string(),
                "n".to_string(),
                "conf".to_string(),
                "out".to_string(),
            ],
        );
        result
    }
}

fn p_adjust_values(p: &[f64], method: &str) -> Vec<f64> {
    let n = p.len();
    if n <= 1 {
        return p.to_vec();
    }
    match method {
        "none" => p.to_vec(),
        "bonferroni" => p.iter().map(|v| (n as f64 * v).min(1.0)).collect(),
        "BH" | "fdr" => {
            let mut o: Vec<usize> = (0..n).collect();
            o.sort_by(|&a, &b| p[b].partial_cmp(&p[a]).unwrap_or(std::cmp::Ordering::Equal));
            let mut adj = vec![0.0; n];
            let mut running: f64 = 1.0;
            for (rank_from_end, &idx) in o.iter().enumerate() {
                let i = n - rank_from_end;
                let val = ((n as f64 / i as f64) * p[idx]).min(1.0);
                running = running.min(val);
                adj[idx] = running;
            }
            adj
        }
        _ => {
            // holm
            let mut o: Vec<usize> = (0..n).collect();
            o.sort_by(|&a, &b| p[a].partial_cmp(&p[b]).unwrap_or(std::cmp::Ordering::Equal));
            let mut adj = vec![0.0; n];
            let mut running: f64 = 0.0;
            for (i, &idx) in o.iter().enumerate() {
                let val = (((n - i) as f64) * p[idx]).min(1.0);
                running = running.max(val);
                adj[idx] = running;
            }
            adj
        }
    }
}

/// GNU `p.adjust(p, method)`.
pub unsafe fn do_p_adjust(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let mut method = "holm".to_string();
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let m = CAR(rest);
            if TYPEOF(m) == SEXPTYPE::STRSXP && XLENGTH(m) > 0 {
                method = elt_to_string(m, 0);
            }
        }
        let n = XLENGTH(x);
        let mut p = Vec::with_capacity(n as usize);
        for i in 0..n {
            p.push(elt_real_safe(x, i));
        }
        let adj = p_adjust_values(&p, &method);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for (i, v) in adj.iter().enumerate() {
            *REAL(result).add(i) = *v;
        }
        result
    }
}

/// GNU one-sample `t.test(x)`.
pub unsafe fn do_t_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n0 = XLENGTH(x);
        let t = TYPEOF(x);
        let mut vals = Vec::new();
        for i in 0..n0 {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            if v.is_finite() {
                vals.push(v);
            }
        }
        let n = vals.len() as f64;
        if n < 2.0 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "not enough 'x' observations",
            );
        }
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n - 1.0);
        let s = var.sqrt();
        let se = s / n.sqrt();
        let stat = mean / se;
        let df = n - 1.0;
        let p = 2.0 * crate::dist::t_dist::pt_inner(-stat.abs(), df, true, false);
        let crit = crate::dist::t_dist::qt_inner(0.975, df, true, false);
        let lo = mean - crit * se;
        let hi = mean + crit * se;

        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["t".to_string()]);
        let parameter = Rf_ScalarReal(df);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let estimate = Rf_ScalarReal(mean);
        let _es = protect(estimate);
        set_string_names(estimate, &["mean of x".to_string()]);
        let null_value = Rf_ScalarReal(0.0);
        let _nv = protect(null_value);
        set_string_names(null_value, &["mean".to_string()]);
        let conf = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _cf = protect(conf);
        *REAL(conf) = lo;
        *REAL(conf).add(1) = hi;
        crate::sexp::attrib_core::setAttrib(
            conf,
            crate::sexp::symbol::Rf_install(c"conf.level".as_ptr()),
            Rf_ScalarReal(0.95),
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 10);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(p));
        SET_VECTOR_ELT(result, 3, conf);
        SET_VECTOR_ELT(result, 4, estimate);
        SET_VECTOR_ELT(result, 5, null_value);
        SET_VECTOR_ELT(result, 6, Rf_ScalarReal(se));
        SET_VECTOR_ELT(result, 7, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(result, 8, Rf_mkString(c"One Sample t-test".as_ptr()));
        SET_VECTOR_ELT(result, 9, Rf_mkString(c"x".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "conf.int".to_string(),
                "estimate".to_string(),
                "null.value".to_string(),
                "stderr".to_string(),
                "alternative".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn binom_two_sided_p(x: f64, n: f64, p: f64) -> f64 {
    let dobs = crate::dist::binomial::dbinom_inner(x, n, p, false);
    let mut s = 0.0;
    let mut k = 0.0;
    while k <= n + 0.5 {
        let dk = crate::dist::binomial::dbinom_inner(k, n, p, false);
        if dk <= dobs * (1.0 + 1e-7) {
            s += dk;
        }
        k += 1.0;
    }
    s.min(1.0)
}

/// GNU `binom.test(x, n, p=0.5)`.
pub unsafe fn do_binom_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = elt_real_safe(CAR(args), 0).round();
        let n_arg = CAR(CDR(args));
        let n = if n_arg.is_null() || n_arg == R_NilValue() {
            x
        } else {
            elt_real_safe(n_arg, 0).round()
        };
        let mut p0 = 0.5;
        let rest = CDR(CDR(args));
        if !rest.is_null() && rest != R_NilValue() {
            let p_s = CAR(rest);
            if !p_s.is_null() && p_s != R_NilValue() && TYPEOF(p_s) != SEXPTYPE::STRSXP {
                p0 = elt_real_safe(p_s, 0);
            }
        }
        let pval = binom_two_sided_p(x, n, p0);
        let alpha = 0.025;
        let lo = if x == 0.0 {
            0.0
        } else {
            crate::dist::beta::qbeta_inner(alpha, x, n - x + 1.0, true, false)
        };
        let hi = if x == n {
            1.0
        } else {
            crate::dist::beta::qbeta_inner(1.0 - alpha, x + 1.0, n - x, true, false)
        };
        let statistic = Rf_ScalarReal(x);
        let _st = protect(statistic);
        set_string_names(statistic, &["number of successes".to_string()]);
        let parameter = Rf_ScalarReal(n);
        let _pa = protect(parameter);
        set_string_names(parameter, &["number of trials".to_string()]);
        let estimate = Rf_ScalarReal(x / n);
        let _es = protect(estimate);
        set_string_names(estimate, &["probability of success".to_string()]);
        let null_value = Rf_ScalarReal(p0);
        let _nv = protect(null_value);
        set_string_names(null_value, &["probability of success".to_string()]);
        let conf = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _cf = protect(conf);
        *REAL(conf) = lo;
        *REAL(conf).add(1) = hi;
        crate::sexp::attrib_core::setAttrib(
            conf,
            crate::sexp::symbol::Rf_install(c"conf.level".as_ptr()),
            Rf_ScalarReal(0.95),
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 9);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, conf);
        SET_VECTOR_ELT(result, 4, estimate);
        SET_VECTOR_ELT(result, 5, null_value);
        SET_VECTOR_ELT(result, 6, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(result, 7, Rf_mkString(c"Exact binomial test".as_ptr()));
        SET_VECTOR_ELT(result, 8, Rf_mkString(c"x and n".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "conf.int".to_string(),
                "estimate".to_string(),
                "null.value".to_string(),
                "alternative".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `chisq.test(x)` goodness-of-fit with equal p.
pub unsafe fn do_chisq_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let k = XLENGTH(x);
        let mut obs = Vec::with_capacity(k as usize);
        let mut n = 0.0;
        for i in 0..k {
            let v = elt_real_safe(x, i);
            obs.push(v);
            n += v;
        }
        let e = n / k as f64;
        let mut stat = 0.0;
        for v in &obs {
            if e > 0.0 {
                let d = *v - e;
                stat += d * d / e;
            }
        }
        let df = (k - 1) as f64;
        let pval = crate::dist::chisq::pchisq_inner(stat, df, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["X-squared".to_string()]);
        let parameter = Rf_ScalarReal(df);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let expected = Rf_allocVector3(SEXPTYPE::REALSXP, k);
        let _ex = protect(expected);
        for i in 0..k {
            *REAL(expected).add(i as usize) = e;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 6);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(
            result,
            3,
            Rf_mkString(c"Chi-squared test for given probabilities".as_ptr()),
        );
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x".as_ptr()));
        SET_VECTOR_ELT(result, 5, expected);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "method".to_string(),
                "data.name".to_string(),
                "expected".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU one-sample `prop.test(x, n)`.
pub unsafe fn do_prop_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = elt_real_safe(CAR(args), 0);
        let n = elt_real_safe(CAR(CDR(args)), 0);
        let p0 = 0.5;
        let estimate = x / n;
        let yates = (0.5_f64).min((x - n * p0).abs());
        let e_s = n * p0;
        let e_f = n * (1.0 - p0);
        let stat = {
            let a = (x - e_s).abs() - yates;
            let b = ((n - x) - e_f).abs() - yates;
            a * a / e_s + b * b / e_f
        };
        let pval = crate::dist::chisq::pchisq_inner(stat, 1.0, false, false);
        let z = crate::dist::normal::qnorm5_inner(0.975, 0.0, 1.0, true, false);
        let z22n = z * z / (2.0 * n);
        let pc_u = estimate + yates / n;
        let pc_l = estimate - yates / n;
        let p_u = if pc_u >= 1.0 {
            1.0
        } else {
            (pc_u + z22n + z * (pc_u * (1.0 - pc_u) / n + z22n / (2.0 * n)).sqrt())
                / (1.0 + 2.0 * z22n)
        };
        let p_l = if pc_l <= 0.0 {
            0.0
        } else {
            (pc_l + z22n - z * (pc_l * (1.0 - pc_l) / n + z22n / (2.0 * n)).sqrt())
                / (1.0 + 2.0 * z22n)
        };
        let method = if yates > 0.0 {
            "1-sample proportions test with continuity correction"
        } else {
            "1-sample proportions test without continuity correction"
        };
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["X-squared".to_string()]);
        let parameter = Rf_ScalarReal(1.0);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let est = Rf_ScalarReal(estimate);
        let _es = protect(est);
        set_string_names(est, &["p".to_string()]);
        let null_value = Rf_ScalarReal(p0);
        let _nv = protect(null_value);
        set_string_names(null_value, &["p".to_string()]);
        let conf = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _cf = protect(conf);
        *REAL(conf) = p_l.max(0.0);
        *REAL(conf).add(1) = p_u.min(1.0);
        crate::sexp::attrib_core::setAttrib(
            conf,
            crate::sexp::symbol::Rf_install(c"conf.level".as_ptr()),
            Rf_ScalarReal(0.95),
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 9);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, est);
        SET_VECTOR_ELT(result, 4, null_value);
        SET_VECTOR_ELT(result, 5, conf);
        SET_VECTOR_ELT(result, 6, Rf_mkString(c"two.sided".as_ptr()));
        let m = CString::new(method).unwrap_or_default();
        SET_VECTOR_ELT(result, 7, Rf_mkString(m.as_ptr()));
        SET_VECTOR_ELT(result, 8, Rf_mkString(c"x out of n".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "estimate".to_string(),
                "null.value".to_string(),
                "conf.int".to_string(),
                "alternative".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn pearson_r(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len() as f64;
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let mut num = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;
    for i in 0..x.len() {
        let dx = x[i] - mx;
        let dy = y[i] - my;
        num += dx * dy;
        sx += dx * dx;
        sy += dy * dy;
    }
    num / (sx * sy).sqrt()
}

/// GNU Pearson `cor.test(x, y)`.
pub unsafe fn do_cor_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let xs = CAR(args);
        let ys = CAR(CDR(args));
        let n0 = XLENGTH(xs).min(XLENGTH(ys));
        let mut x = Vec::new();
        let mut y = Vec::new();
        for i in 0..n0 {
            let a = elt_real_safe(xs, i);
            let b = elt_real_safe(ys, i);
            if a.is_finite() && b.is_finite() {
                x.push(a);
                y.push(b);
            }
        }
        let n = x.len() as f64;
        let r = pearson_r(&x, &y);
        let df = n - 2.0;
        let stat = df.sqrt() * r / (1.0 - r * r).sqrt();
        let p1 = crate::dist::t_dist::pt_inner(stat, df, true, false);
        let p2 = crate::dist::t_dist::pt_inner(stat, df, false, false);
        let pval = (2.0 * p1.min(p2)).min(1.0);
        let z = r.atanh();
        let sigma = 1.0 / (n - 3.0).sqrt();
        let qz = crate::dist::normal::qnorm5_inner(0.975, 0.0, 1.0, true, false);
        let lo = (z - sigma * qz).tanh();
        let hi = (z + sigma * qz).tanh();
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["t".to_string()]);
        let parameter = Rf_ScalarReal(df);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let estimate = Rf_ScalarReal(r);
        let _es = protect(estimate);
        set_string_names(estimate, &["cor".to_string()]);
        let null_value = Rf_ScalarReal(0.0);
        let _nv = protect(null_value);
        set_string_names(null_value, &["correlation".to_string()]);
        let conf = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _cf = protect(conf);
        *REAL(conf) = lo;
        *REAL(conf).add(1) = hi;
        crate::sexp::attrib_core::setAttrib(
            conf,
            crate::sexp::symbol::Rf_install(c"conf.level".as_ptr()),
            Rf_ScalarReal(0.95),
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 9);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, estimate);
        SET_VECTOR_ELT(result, 4, null_value);
        SET_VECTOR_ELT(result, 5, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(
            result,
            6,
            Rf_mkString(c"Pearson's product-moment correlation".as_ptr()),
        );
        SET_VECTOR_ELT(result, 7, Rf_mkString(c"x and y".as_ptr()));
        SET_VECTOR_ELT(result, 8, conf);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "estimate".to_string(),
                "null.value".to_string(),
                "alternative".to_string(),
                "method".to_string(),
                "data.name".to_string(),
                "conf.int".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn sample_var(x: &[f64]) -> f64 {
    let n = x.len() as f64;
    let m = x.iter().sum::<f64>() / n;
    x.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (n - 1.0)
}

/// GNU `var.test(x, y)`.
pub unsafe fn do_var_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let xs = CAR(args);
        let ys = CAR(CDR(args));
        let mut x = Vec::new();
        let mut y = Vec::new();
        for i in 0..XLENGTH(xs) {
            let v = elt_real_safe(xs, i);
            if v.is_finite() {
                x.push(v);
            }
        }
        for i in 0..XLENGTH(ys) {
            let v = elt_real_safe(ys, i);
            if v.is_finite() {
                y.push(v);
            }
        }
        let dfx = (x.len() - 1) as f64;
        let dfy = (y.len() - 1) as f64;
        let vx = sample_var(&x);
        let vy = sample_var(&y);
        let est = vx / vy;
        let stat = est;
        let p_lo = crate::dist::f_dist::pf_inner(stat, dfx, dfy, true, false);
        let pval = (2.0 * p_lo.min(1.0 - p_lo)).min(1.0);
        let lo = est / crate::dist::f_dist::qf_inner(0.975, dfx, dfy, true, false);
        let hi = est / crate::dist::f_dist::qf_inner(0.025, dfx, dfy, true, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["F".to_string()]);
        let parameter = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _pa = protect(parameter);
        *REAL(parameter) = dfx;
        *REAL(parameter).add(1) = dfy;
        set_string_names(parameter, &["num df".to_string(), "denom df".to_string()]);
        let estimate = Rf_ScalarReal(est);
        let _es = protect(estimate);
        set_string_names(estimate, &["ratio of variances".to_string()]);
        let null_value = Rf_ScalarReal(1.0);
        let _nv = protect(null_value);
        set_string_names(null_value, &["ratio of variances".to_string()]);
        let conf = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _cf = protect(conf);
        *REAL(conf) = lo;
        *REAL(conf).add(1) = hi;
        crate::sexp::attrib_core::setAttrib(
            conf,
            crate::sexp::symbol::Rf_install(c"conf.level".as_ptr()),
            Rf_ScalarReal(0.95),
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 9);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, conf);
        SET_VECTOR_ELT(result, 4, estimate);
        SET_VECTOR_ELT(result, 5, null_value);
        SET_VECTOR_ELT(result, 6, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(
            result,
            7,
            Rf_mkString(c"F test to compare two variances".as_ptr()),
        );
        SET_VECTOR_ELT(result, 8, Rf_mkString(c"x and y".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "conf.int".to_string(),
                "estimate".to_string(),
                "null.value".to_string(),
                "alternative".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `bartlett.test(x, g)`.
pub unsafe fn do_bartlett_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let g = CAR(CDR(args));
        let n0 = XLENGTH(x);
        let mut groups: std::collections::BTreeMap<i32, Vec<f64>> =
            std::collections::BTreeMap::new();
        for i in 0..n0 {
            let v = elt_real_safe(x, i);
            if !v.is_finite() {
                continue;
            }
            let gi = if g.is_null() || g == R_NilValue() {
                1
            } else if TYPEOF(g) == SEXPTYPE::INTSXP || TYPEOF(g) == SEXPTYPE::LGLSXP {
                *INTEGER(g).add((i as usize) % XLENGTH(g) as usize)
            } else if TYPEOF(g) == SEXPTYPE::REALSXP {
                *REAL(g).add((i as usize) % XLENGTH(g) as usize) as i32
            } else {
                1
            };
            groups.entry(gi).or_default().push(v);
        }
        let k = groups.len();
        let mut ns: Vec<f64> = Vec::new();
        let mut vs: Vec<f64> = Vec::new();
        for vals in groups.values() {
            if vals.len() < 2 {
                continue;
            }
            ns.push((vals.len() - 1) as f64);
            vs.push(sample_var(vals));
        }
        let n_total: f64 = ns.iter().sum();
        let v_total = ns.iter().zip(vs.iter()).map(|(n, v)| n * v).sum::<f64>() / n_total;
        let num = n_total * v_total.ln()
            - ns.iter()
                .zip(vs.iter())
                .map(|(n, v)| n * v.ln())
                .sum::<f64>();
        let den = 1.0
            + (ns.iter().map(|n| 1.0 / n).sum::<f64>() - 1.0 / n_total) / (3.0 * (k as f64 - 1.0));
        let stat = num / den;
        let df = (k as f64) - 1.0;
        let pval = crate::dist::chisq::pchisq_inner(stat, df, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Bartlett's K-squared".to_string()]);
        let parameter = Rf_ScalarReal(df);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, Rf_mkString(c"x and g".as_ptr()));
        SET_VECTOR_ELT(
            result,
            4,
            Rf_mkString(c"Bartlett test of homogeneity of variances".as_ptr()),
        );
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "data.name".to_string(),
                "method".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn rank_average(x: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..x.len()).collect();
    idx.sort_by(|&a, &b| x[a].partial_cmp(&x[b]).unwrap_or(std::cmp::Ordering::Equal));
    let mut ranks = vec![0.0; x.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i + 1;
        while j < idx.len() && x[idx[j]] == x[idx[i]] {
            j += 1;
        }
        let avg = ((i + 1) + j) as f64 / 2.0;
        for k in i..j {
            ranks[idx[k]] = avg;
        }
        i = j;
    }
    ranks
}

/// GNU `kruskal.test(x, g)`.
pub unsafe fn do_kruskal_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let g = CAR(CDR(args));
        let n0 = XLENGTH(x);
        let mut vals = Vec::new();
        let mut groups = Vec::new();
        for i in 0..n0 {
            let v = elt_real_safe(x, i);
            if !v.is_finite() {
                continue;
            }
            let gi = if g.is_null() || g == R_NilValue() {
                1
            } else if TYPEOF(g) == SEXPTYPE::INTSXP || TYPEOF(g) == SEXPTYPE::LGLSXP {
                *INTEGER(g).add((i as usize) % XLENGTH(g) as usize)
            } else if TYPEOF(g) == SEXPTYPE::REALSXP {
                *REAL(g).add((i as usize) % XLENGTH(g) as usize) as i32
            } else {
                1
            };
            vals.push(v);
            groups.push(gi);
        }
        let ranks = rank_average(&vals);
        let n = vals.len() as f64;
        let mut by_g: std::collections::BTreeMap<i32, (f64, f64)> =
            std::collections::BTreeMap::new();
        for (r, g) in ranks.iter().zip(groups.iter()) {
            let e = by_g.entry(*g).or_insert((0.0, 0.0));
            e.0 += *r;
            e.1 += 1.0;
        }
        let k = by_g.len();
        let sum_r2_n: f64 = by_g.values().map(|(sr, ng)| sr * sr / ng).sum();
        let mut tie_adj = 0.0;
        let mut counts: std::collections::BTreeMap<u64, f64> = std::collections::BTreeMap::new();
        for v in &vals {
            *counts.entry(v.to_bits()).or_insert(0.0) += 1.0;
        }
        for t in counts.values() {
            tie_adj += t * t * t - t;
        }
        let den = 1.0 - tie_adj / (n * n * n - n);
        let stat = (12.0 * sum_r2_n / (n * (n + 1.0)) - 3.0 * (n + 1.0)) / den;
        let df = (k as f64) - 1.0;
        let pval = crate::dist::chisq::pchisq_inner(stat, df, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Kruskal-Wallis chi-squared".to_string()]);
        let parameter = Rf_ScalarReal(df);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(
            result,
            3,
            Rf_mkString(c"Kruskal-Wallis rank sum test".as_ptr()),
        );
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x and g".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn median_of(x: &[f64]) -> f64 {
    let mut s = x.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = s.len();
    if n == 0 {
        return f64::NAN;
    }
    if n % 2 == 1 {
        s[n / 2]
    } else {
        0.5 * (s[n / 2 - 1] + s[n / 2])
    }
}

/// GNU `fligner.test(x, g)`.
pub unsafe fn do_fligner_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let g = CAR(CDR(args));
        let n0 = XLENGTH(x);
        let mut vals = Vec::new();
        let mut groups = Vec::new();
        for i in 0..n0 {
            let v = elt_real_safe(x, i);
            if !v.is_finite() {
                continue;
            }
            let gi = if g.is_null() || g == R_NilValue() {
                1
            } else if TYPEOF(g) == SEXPTYPE::INTSXP || TYPEOF(g) == SEXPTYPE::LGLSXP {
                *INTEGER(g).add((i as usize) % XLENGTH(g) as usize)
            } else if TYPEOF(g) == SEXPTYPE::REALSXP {
                *REAL(g).add((i as usize) % XLENGTH(g) as usize) as i32
            } else {
                1
            };
            vals.push(v);
            groups.push(gi);
        }
        let mut by_g: std::collections::BTreeMap<i32, Vec<f64>> = std::collections::BTreeMap::new();
        for (v, g) in vals.iter().zip(groups.iter()) {
            by_g.entry(*g).or_default().push(*v);
        }
        let medians: std::collections::BTreeMap<i32, f64> =
            by_g.iter().map(|(k, v)| (*k, median_of(v))).collect();
        let centered: Vec<f64> = vals
            .iter()
            .zip(groups.iter())
            .map(|(v, g)| v - medians[g])
            .collect();
        let abs_c: Vec<f64> = centered.iter().map(|v| v.abs()).collect();
        let ranks = rank_average(&abs_c);
        let n = vals.len() as f64;
        let mut a: Vec<f64> = ranks
            .iter()
            .map(|r| {
                crate::dist::normal::qnorm5_inner(
                    (1.0 + r / (n + 1.0)) / 2.0,
                    0.0,
                    1.0,
                    true,
                    false,
                )
            })
            .collect();
        let mean_a = a.iter().sum::<f64>() / n;
        for v in &mut a {
            *v -= mean_a;
        }
        let vsum = a.iter().map(|v| v * v).sum::<f64>() / (n - 1.0);
        let mut a_by_g: std::collections::BTreeMap<i32, Vec<f64>> =
            std::collections::BTreeMap::new();
        for (ai, g) in a.iter().zip(groups.iter()) {
            a_by_g.entry(*g).or_default().push(*ai);
        }
        let k = a_by_g.len();
        let stat = a_by_g
            .values()
            .map(|ag| {
                let m = ag.iter().sum::<f64>() / ag.len() as f64;
                (ag.len() as f64) * m * m
            })
            .sum::<f64>()
            / vsum;
        let df = (k as f64) - 1.0;
        let pval = crate::dist::chisq::pchisq_inner(stat, df, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Fligner-Killeen:med chi-squared".to_string()]);
        let parameter = Rf_ScalarReal(df);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(
            result,
            3,
            Rf_mkString(c"Fligner-Killeen test of homogeneity of variances".as_ptr()),
        );
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x and g".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `mood.test(x, y)`.
pub unsafe fn do_mood_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let xs = CAR(args);
        let ys = CAR(CDR(args));
        let mut x = Vec::new();
        let mut y = Vec::new();
        for i in 0..XLENGTH(xs) {
            let v = elt_real_safe(xs, i);
            if v.is_finite() {
                x.push(v);
            }
        }
        for i in 0..XLENGTH(ys) {
            let v = elt_real_safe(ys, i);
            if v.is_finite() {
                y.push(v);
            }
        }
        let m = x.len() as f64;
        let n = y.len() as f64;
        let ntot = m + n;
        let mut z = x.clone();
        z.extend_from_slice(&y);
        let mut has_ties = false;
        let mut seen = std::collections::BTreeSet::new();
        for v in &z {
            if !seen.insert(v.to_bits()) {
                has_ties = true;
                break;
            }
        }
        let e = m * (ntot * ntot - 1.0) / 12.0;
        let mut v = m * n * (ntot + 1.0) * (ntot + 2.0) * (ntot - 2.0) / 180.0;
        let tstat = if !has_ties {
            let ranks = rank_average(&z);
            let mid = (ntot + 1.0) / 2.0;
            ranks
                .iter()
                .take(x.len())
                .map(|r| (r - mid) * (r - mid))
                .sum::<f64>()
        } else {
            let mut u = z.clone();
            u.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            u.dedup_by(|a, b| a.to_bits() == b.to_bits());
            let mut a_counts = vec![0.0; u.len()];
            let mut t_counts = vec![0.0; u.len()];
            for xv in &x {
                if let Some(j) = u.iter().position(|uv| uv.to_bits() == xv.to_bits()) {
                    a_counts[j] += 1.0;
                }
            }
            for zv in &z {
                if let Some(j) = u.iter().position(|uv| uv.to_bits() == zv.to_bits()) {
                    t_counts[j] += 1.0;
                }
            }
            let mid = (ntot + 1.0) / 2.0;
            let mut p = vec![0.0; ntot as usize];
            let mut acc = 0.0;
            for i in 0..(ntot as usize) {
                let d = (i as f64 + 1.0) - mid;
                acc += d * d;
                p[i] = acc;
            }
            let mut csum = 0.0;
            let mut p_at = Vec::new();
            let mut cums = Vec::new();
            for t in &t_counts {
                csum += *t;
                cums.push(csum);
                p_at.push(p[(csum as usize) - 1]);
            }
            let mut prev = 0.0;
            let mut tstat = 0.0;
            let mut sum_term = 0.0;
            for (j, t) in t_counts.iter().enumerate() {
                let block = p_at[j] - prev;
                prev = p_at[j];
                tstat += a_counts[j] * block / t;
                let cs = cums[j];
                let inner = t * t - 4.0 + 15.0 * (ntot - 2.0 * cs + t).powi(2);
                sum_term += t * (t * t - 1.0) * inner;
            }
            v -= (m * n) / (180.0 * ntot * (ntot - 1.0)) * sum_term;
            tstat
        };
        let zstat = (tstat - e) / v.sqrt();
        let p = crate::dist::normal::pnorm5_inner(zstat, 0.0, 1.0, true, false);
        let pval = (2.0 * p.min(1.0 - p)).min(1.0);
        let statistic = Rf_ScalarReal(zstat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Z".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 2, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(
            result,
            3,
            Rf_mkString(c"Mood two-sample test of scale".as_ptr()),
        );
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x and y".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "p.value".to_string(),
                "alternative".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `ansari.test(x, y)` asymptotic.
pub unsafe fn do_ansari_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let xs = CAR(args);
        let ys = CAR(CDR(args));
        let mut x = Vec::new();
        let mut y = Vec::new();
        for i in 0..XLENGTH(xs) {
            let v = elt_real_safe(xs, i);
            if v.is_finite() {
                x.push(v);
            }
        }
        for i in 0..XLENGTH(ys) {
            let v = elt_real_safe(ys, i);
            if v.is_finite() {
                y.push(v);
            }
        }
        let m = x.len() as f64;
        let n = y.len() as f64;
        let ntot = m + n;
        let mut z = x.clone();
        z.extend_from_slice(&y);
        let ranks = rank_average(&z);
        let scores: Vec<f64> = ranks.iter().map(|r| r.min(ntot - r + 1.0)).collect();
        let stat: f64 = scores.iter().take(x.len()).sum();
        let even = (ntot as i64) % 2 == 0;
        let mut unique = z.clone();
        unique.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        unique.dedup_by(|a, b| a.to_bits() == b.to_bits());
        let ties = unique.len() != z.len();
        let (zz, sigma) = if !ties {
            if even {
                (
                    stat - m * (ntot + 2.0) / 4.0,
                    (m * n * (ntot + 2.0) * (ntot - 2.0) / (48.0 * (ntot - 1.0))).sqrt(),
                )
            } else {
                (
                    stat - m * (ntot + 1.0).powi(2) / (4.0 * ntot),
                    (m * n * (ntot + 1.0) * (3.0 + ntot * ntot) / (48.0 * ntot * ntot)).sqrt(),
                )
            }
        } else {
            let mean_a = scores.iter().sum::<f64>() / ntot;
            let var_a = scores
                .iter()
                .map(|a| (a - mean_a) * (a - mean_a))
                .sum::<f64>()
                / (ntot - 1.0);
            (stat - m * mean_a, (m * n * var_a / ntot).sqrt())
        };
        let p = crate::dist::normal::pnorm5_inner(zz / sigma, 0.0, 1.0, true, false);
        let pval = (2.0 * p.min(1.0 - p)).min(1.0);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["AB".to_string()]);
        let null_value = Rf_ScalarReal(1.0);
        let _nv = protect(null_value);
        set_string_names(null_value, &["ratio of scales".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 6);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 2, null_value);
        SET_VECTOR_ELT(result, 3, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"Ansari-Bradley test".as_ptr()));
        SET_VECTOR_ELT(result, 5, Rf_mkString(c"x and y".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "p.value".to_string(),
                "null.value".to_string(),
                "alternative".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU 2x2 `mcnemar.test(x, correct=TRUE)`.
pub unsafe fn do_mcnemar_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || XLENGTH(x) < 4 {
            return R_NilValue();
        }
        let mut correct = true;
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let c = CAR(rest);
            if TYPEOF(c) == SEXPTYPE::LGLSXP && XLENGTH(c) > 0 {
                correct = *INTEGER(c) != 0;
            }
        }
        let a12 = elt_real_safe(x, 2);
        let a21 = elt_real_safe(x, 1);
        let off = a12 + a21;
        let y = if correct && (a12 - a21).abs() > 0.0 {
            (a12 - a21).abs() - 1.0
        } else {
            a12 - a21
        };
        let stat = if off > 0.0 { y * y / off } else { 0.0 };
        let pval = crate::dist::chisq::pchisq_inner(stat, 1.0, false, false);
        let method = if correct && (a12 - a21).abs() > 0.0 {
            "McNemar's Chi-squared test with continuity correction"
        } else {
            "McNemar's Chi-squared test"
        };
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["McNemar's chi-squared".to_string()]);
        let parameter = Rf_ScalarReal(1.0);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        let m = CString::new(method).unwrap_or_default();
        SET_VECTOR_ELT(result, 3, Rf_mkString(m.as_ptr()));
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU 2x2 `fisher.test(x)`.
pub unsafe fn do_fisher_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let tab = CAR(args);
        if tab.is_null() || tab == R_NilValue() || XLENGTH(tab) < 4 {
            return R_NilValue();
        }
        let a11 = elt_real_safe(tab, 0);
        let a21 = elt_real_safe(tab, 1);
        let a12 = elt_real_safe(tab, 2);
        let a22 = elt_real_safe(tab, 3);
        let m = a11 + a21;
        let n = a12 + a22;
        let k = a11 + a12;
        let x = a11;
        let lo = 0.0_f64.max(k - n);
        let hi = k.min(m);
        let dobs = crate::dist::hypergeometric::dhyper_inner(x, m, n, k, false);
        let mut pval = 0.0;
        let mut t = lo;
        while t <= hi + 0.5 {
            let dt = crate::dist::hypergeometric::dhyper_inner(t, m, n, k, false);
            if dt <= dobs * (1.0 + 1e-7) {
                pval += dt;
            }
            t += 1.0;
        }
        pval = pval.min(1.0);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(pval));
        let nv = Rf_ScalarReal(1.0);
        let _nv = protect(nv);
        set_string_names(nv, &["odds ratio".to_string()]);
        SET_VECTOR_ELT(result, 1, nv);
        SET_VECTOR_ELT(result, 2, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(
            result,
            3,
            Rf_mkString(c"Fisher's Exact Test for Count Data".as_ptr()),
        );
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "p.value".to_string(),
                "null.value".to_string(),
                "alternative".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU unreplicated-block `friedman.test(x)`.
pub unsafe fn do_friedman_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (n, k) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            return R_NilValue();
        };
        if n < 2 || k < 2 {
            return R_NilValue();
        }
        let mut ranks = vec![0.0; n * k];
        let mut tie_sum = 0.0;
        for i in 0..n {
            let mut row = Vec::with_capacity(k);
            for j in 0..k {
                row.push(elt_real_safe(x, (j * n + i) as i64));
            }
            let rr = rank_average(&row);
            let mut counts: Vec<(f64, i32)> = Vec::new();
            for (j, &r) in rr.iter().enumerate() {
                ranks[j * n + i] = r;
                if let Some(c) = counts.iter_mut().find(|(v, _)| *v == r) {
                    c.1 += 1;
                } else {
                    counts.push((r, 1));
                }
            }
            for (_, u) in counts {
                let uf = u as f64;
                tie_sum += uf * uf * uf - uf;
            }
        }
        let expect = n as f64 * (k as f64 + 1.0) / 2.0;
        let mut ss = 0.0;
        for j in 0..k {
            let mut s = 0.0;
            for i in 0..n {
                s += ranks[j * n + i];
            }
            let d = s - expect;
            ss += d * d;
        }
        let denom = (n * k * (k + 1)) as f64 - tie_sum / (k as f64 - 1.0);
        let stat = if denom > 0.0 { 12.0 * ss / denom } else { 0.0 };
        let df = (k - 1) as f64;
        let pval = crate::dist::chisq::pchisq_inner(stat, df, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Friedman chi-squared".to_string()]);
        let parameter = Rf_ScalarReal(df);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, Rf_mkString(c"Friedman rank sum test".as_ptr()));
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU unreplicated-block `quade.test(x)`.
pub unsafe fn do_quade_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (b, k) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            return R_NilValue();
        };
        if b < 2 || k < 2 {
            return R_NilValue();
        }
        let mid = (k as f64 + 1.0) / 2.0;
        let mut ranges = vec![0.0; b];
        let mut s = vec![0.0; b * k];
        for i in 0..b {
            let mut row = Vec::with_capacity(k);
            for j in 0..k {
                row.push(elt_real_safe(x, (j * b + i) as i64));
            }
            let rr = rank_average(&row);
            let mut mn = f64::INFINITY;
            let mut mx = f64::NEG_INFINITY;
            for &v in &row {
                mn = mn.min(v);
                mx = mx.max(v);
            }
            ranges[i] = mx - mn;
            for j in 0..k {
                s[j * b + i] = rr[j] - mid;
            }
        }
        let q = rank_average(&ranges);
        let mut a = 0.0;
        let mut col = vec![0.0; k];
        for i in 0..b {
            for j in 0..k {
                let v = q[i] * s[j * b + i];
                s[j * b + i] = v;
                a += v * v;
                col[j] += v;
            }
        }
        let mut bss = 0.0;
        for c in col {
            bss += c * c;
        }
        let bb = bss / (b as f64);
        let stat = if (a - bb).abs() < 1e-15 {
            f64::NAN
        } else {
            (b as f64 - 1.0) * bb / (a - bb)
        };
        let df1 = (k - 1) as f64;
        let df2 = ((b - 1) * (k - 1)) as f64;
        let pval = crate::dist::f_dist::pf_inner(stat, df1, df2, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Quade F".to_string()]);
        let parameter = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _pa = protect(parameter);
        *REAL(parameter) = df1;
        *REAL(parameter).add(1) = df2;
        set_string_names(parameter, &["num df".to_string(), "denom df".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, Rf_mkString(c"Quade test".as_ptr()));
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU 2x2xK `mantelhaen.test(x)` (asymptotic, two-sided).
pub unsafe fn do_mantelhaen_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null()
            || dim == R_NilValue()
            || TYPEOF(dim) != SEXPTYPE::INTSXP
            || XLENGTH(dim) < 3
        {
            return R_NilValue();
        }
        let i = *INTEGER(dim) as usize;
        let j = *INTEGER(dim).add(1) as usize;
        let k = *INTEGER(dim).add(2) as usize;
        if i != 2 || j != 2 || k < 1 {
            return R_NilValue();
        }
        let mut delta = 0.0;
        let mut varsum = 0.0;
        let mut s_diag = 0.0;
        let mut s_offd = 0.0;
        for s in 0..k {
            let a = elt_real_safe(x, (s * 4) as i64);
            let b = elt_real_safe(x, (s * 4 + 1) as i64);
            let c = elt_real_safe(x, (s * 4 + 2) as i64);
            let d = elt_real_safe(x, (s * 4 + 3) as i64);
            let n = a + b + c + d;
            if n <= 1.0 {
                continue;
            }
            let sx1 = a + c;
            let sx2 = b + d;
            let sy1 = a + b;
            let sy2 = c + d;
            delta += a - sx1 * sy1 / n;
            varsum += sx1 * sx2 * sy1 * sy2 / (n * n * (n - 1.0));
            s_diag += a * d / n;
            s_offd += c * b / n;
        }
        let yates = if delta.abs() >= 0.5 { 0.5 } else { 0.0 };
        let stat = if varsum > 0.0 {
            let num = delta.abs() - yates;
            num * num / varsum
        } else {
            0.0
        };
        let pval = crate::dist::chisq::pchisq_inner(stat, 1.0, false, false);
        let estimate = if s_offd > 0.0 {
            s_diag / s_offd
        } else {
            f64::INFINITY
        };
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Mantel-Haenszel X-squared".to_string()]);
        let parameter = Rf_ScalarReal(1.0);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let est = Rf_ScalarReal(estimate);
        let _es = protect(est);
        set_string_names(est, &["common odds ratio".to_string()]);
        let method_s = if yates > 0.0 {
            Rf_mkString(c"Mantel-Haenszel chi-squared test with continuity correction".as_ptr())
        } else {
            Rf_mkString(c"Mantel-Haenszel chi-squared test without continuity correction".as_ptr())
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 6);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, est);
        SET_VECTOR_ELT(result, 4, method_s);
        SET_VECTOR_ELT(result, 5, Rf_mkString(c"x".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "estimate".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `pairwise.t.test(x, g)` with pooled SD.
pub unsafe fn do_pairwise_t_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let g = CAR(CDR(args));
        let mut method = "holm".to_string();
        let mut cell = CDR(CDR(args));
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "p.adjust.method" || name.is_empty() {
                let v = CAR(cell);
                if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                    method = elt_to_string(v, 0);
                }
            }
            cell = CDR(cell);
        }
        if x.is_null() || x == R_NilValue() || g.is_null() || g == R_NilValue() {
            return R_NilValue();
        }
        let n0 = XLENGTH(x).min(XLENGTH(g));
        let mut pairs: Vec<(i32, f64)> = Vec::new();
        for i in 0..n0 {
            let gi = elt_real_safe(g, i);
            let xi = elt_real_safe(x, i);
            if gi.is_finite() && xi.is_finite() {
                pairs.push((gi.round() as i32, xi));
            }
        }
        let mut levels: Vec<i32> = pairs.iter().map(|p| p.0).collect();
        levels.sort_unstable();
        levels.dedup();
        let k = levels.len();
        if k < 2 {
            return R_NilValue();
        }
        let mut means = vec![0.0; k];
        let mut vars = vec![0.0; k];
        let mut ns = vec![0.0; k];
        for (li, &lev) in levels.iter().enumerate() {
            let vs: Vec<f64> = pairs.iter().filter(|p| p.0 == lev).map(|p| p.1).collect();
            let n = vs.len() as f64;
            ns[li] = n;
            let m = vs.iter().sum::<f64>() / n;
            means[li] = m;
            vars[li] = vs.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (n - 1.0);
        }
        let total_df: f64 = ns.iter().map(|n| n - 1.0).sum();
        let pooled = (ns
            .iter()
            .zip(vars.iter())
            .map(|(n, v)| v * (n - 1.0))
            .sum::<f64>()
            / total_df)
            .sqrt();
        let mut raw = Vec::new();
        for i in 1..k {
            for j in 0..i {
                let se = pooled * (1.0 / ns[i] + 1.0 / ns[j]).sqrt();
                let t = (means[i] - means[j]) / se;
                let p = 2.0 * crate::dist::t_dist::pt_inner(-t.abs(), total_df, true, false);
                raw.push(p);
            }
        }
        let adj = p_adjust_values(&raw, &method);
        // lower-tri including diag of (k-1) x (k-1), column-major
        let mdim = k - 1;
        let pmat = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            mdim as i32,
            mdim as i32,
        );
        let _pm = protect(pmat);
        for idx in 0..(mdim * mdim) {
            *REAL(pmat).add(idx) = NA_REAL;
        }
        let mut t = 0usize;
        for j in 0..mdim {
            for i in 0..mdim {
                if i >= j {
                    *REAL(pmat).add(i + j * mdim) = adj[t];
                    t += 1;
                }
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_mkString(c"t tests with pooled SD".as_ptr()));
        SET_VECTOR_ELT(result, 1, pmat);
        let meth = if method == "none" {
            Rf_mkString(c"none".as_ptr())
        } else {
            Rf_mkString(c"holm".as_ptr())
        };
        SET_VECTOR_ELT(result, 2, meth);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "method".to_string(),
                "p.value".to_string(),
                "p.adjust.method".to_string(),
            ],
        );
        let class = Rf_mkString(c"pairwise.htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn prop_test_two(x1: f64, n1: f64, x2: f64, n2: f64) -> f64 {
    let p = (x1 + x2) / (n1 + n2);
    let e11 = n1 * p;
    let e12 = n1 * (1.0 - p);
    let e21 = n2 * p;
    let e22 = n2 * (1.0 - p);
    let delta = (x1 / n1 - x2 / n2).abs();
    let yates = 0.5_f64.min(delta / (1.0 / n1 + 1.0 / n2));
    let stat = (x1 - e11).abs() - yates;
    let stat = stat * stat / e11
        + ((n1 - x1 - e12).abs() - yates).powi(2) / e12
        + ((x2 - e21).abs() - yates).powi(2) / e21
        + ((n2 - x2 - e22).abs() - yates).powi(2) / e22;
    crate::dist::chisq::pchisq_inner(stat, 1.0, false, false)
}

/// GNU `pairwise.prop.test(x, n)` with Holm.
pub unsafe fn do_pairwise_prop_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = CAR(CDR(args));
        let mut method = "holm".to_string();
        let mut cell = CDR(CDR(args));
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "p.adjust.method" || name.is_empty() {
                let v = CAR(cell);
                if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                    method = elt_to_string(v, 0);
                }
            }
            cell = CDR(cell);
        }
        if x.is_null() || x == R_NilValue() || n.is_null() || n == R_NilValue() {
            return R_NilValue();
        }
        let k = XLENGTH(x).min(XLENGTH(n)) as usize;
        if k < 2 {
            return R_NilValue();
        }
        let mut xs = Vec::with_capacity(k);
        let mut ns = Vec::with_capacity(k);
        for i in 0..k {
            xs.push(elt_real_safe(x, i as i64));
            ns.push(elt_real_safe(n, i as i64));
        }
        let mut raw = Vec::new();
        for i in 1..k {
            for j in 0..i {
                raw.push(prop_test_two(xs[j], ns[j], xs[i], ns[i]));
            }
        }
        let adj = p_adjust_values(&raw, &method);
        let mdim = k - 1;
        let pmat = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            mdim as i32,
            mdim as i32,
        );
        let _pm = protect(pmat);
        for idx in 0..(mdim * mdim) {
            *REAL(pmat).add(idx) = NA_REAL;
        }
        let mut t = 0usize;
        for j in 0..mdim {
            for i in 0..mdim {
                if i >= j {
                    *REAL(pmat).add(i + j * mdim) = adj[t];
                    t += 1;
                }
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(
            result,
            0,
            Rf_mkString(c"Pairwise comparison of proportions".as_ptr()),
        );
        SET_VECTOR_ELT(result, 1, pmat);
        let meth = if method == "none" {
            Rf_mkString(c"none".as_ptr())
        } else {
            Rf_mkString(c"holm".as_ptr())
        };
        SET_VECTOR_ELT(result, 2, meth);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "method".to_string(),
                "p.value".to_string(),
                "p.adjust.method".to_string(),
            ],
        );
        let class = Rf_mkString(c"pairwise.htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn wilcox_two_asymp(x: &[f64], y: &[f64]) -> f64 {
    let nx = x.len() as f64;
    let ny = y.len() as f64;
    if nx < 1.0 || ny < 1.0 {
        return f64::NAN;
    }
    let mut vals: Vec<(f64, u8)> = x.iter().map(|v| (*v, 0u8)).collect();
    vals.extend(y.iter().map(|v| (*v, 1u8)));
    vals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let n = vals.len();
    let mut ranks = vec![0.0; n];
    let mut i = 0;
    let mut nties3 = 0.0;
    while i < n {
        let mut j = i + 1;
        while j < n && vals[j].0 == vals[i].0 {
            j += 1;
        }
        let r = (i + 1 + j) as f64 / 2.0;
        let u = (j - i) as f64;
        nties3 += u * u * u - u;
        for k in i..j {
            ranks[k] = r;
        }
        i = j;
    }
    let mut wx = 0.0;
    for (k, (_, which)) in vals.iter().enumerate() {
        if *which == 0 {
            wx += ranks[k];
        }
    }
    let w = wx - nx * (nx + 1.0) / 2.0;
    let nxy = nx + ny;
    let sigma = ((nx * ny / 12.0) * ((nxy + 1.0) - nties3 / (nxy * (nxy - 1.0)))).sqrt();
    let mut z = w - nx * ny / 2.0;
    if z != 0.0 {
        z -= z.signum() * 0.5;
    }
    z /= sigma;
    let p = crate::dist::normal::pnorm5_inner(z, 0.0, 1.0, true, false);
    (2.0 * p.min(1.0 - p)).min(1.0)
}

/// GNU `pairwise.wilcox.test(x, g)` asymptotic with continuity.
pub unsafe fn do_pairwise_wilcox_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let g = CAR(CDR(args));
        let mut method = "holm".to_string();
        let mut cell = CDR(CDR(args));
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "p.adjust.method" || name.is_empty() {
                let v = CAR(cell);
                if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                    method = elt_to_string(v, 0);
                }
            }
            cell = CDR(cell);
        }
        if x.is_null() || x == R_NilValue() || g.is_null() || g == R_NilValue() {
            return R_NilValue();
        }
        let n0 = XLENGTH(x).min(XLENGTH(g));
        let mut pairs: Vec<(i32, f64)> = Vec::new();
        for i in 0..n0 {
            let gi = elt_real_safe(g, i);
            let xi = elt_real_safe(x, i);
            if gi.is_finite() && xi.is_finite() {
                pairs.push((gi.round() as i32, xi));
            }
        }
        let mut levels: Vec<i32> = pairs.iter().map(|p| p.0).collect();
        levels.sort_unstable();
        levels.dedup();
        let k = levels.len();
        if k < 2 {
            return R_NilValue();
        }
        let groups: Vec<Vec<f64>> = levels
            .iter()
            .map(|lev| pairs.iter().filter(|p| p.0 == *lev).map(|p| p.1).collect())
            .collect();
        let mut raw = Vec::new();
        for i in 1..k {
            for j in 0..i {
                raw.push(wilcox_two_asymp(&groups[j], &groups[i]));
            }
        }
        let adj = p_adjust_values(&raw, &method);
        let mdim = k - 1;
        let pmat = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            mdim as i32,
            mdim as i32,
        );
        let _pm = protect(pmat);
        for idx in 0..(mdim * mdim) {
            *REAL(pmat).add(idx) = NA_REAL;
        }
        let mut t = 0usize;
        for j in 0..mdim {
            for i in 0..mdim {
                if i >= j {
                    *REAL(pmat).add(i + j * mdim) = adj[t];
                    t += 1;
                }
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(
            result,
            0,
            Rf_mkString(c"Wilcoxon rank sum test with continuity correction".as_ptr()),
        );
        SET_VECTOR_ELT(result, 1, pmat);
        let meth = if method == "none" {
            Rf_mkString(c"none".as_ptr())
        } else {
            Rf_mkString(c"holm".as_ptr())
        };
        SET_VECTOR_ELT(result, 2, meth);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "method".to_string(),
                "p.value".to_string(),
                "p.adjust.method".to_string(),
            ],
        );
        let class = Rf_mkString(c"pairwise.htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn invert3(a: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    if !det.is_finite() || det.abs() < 1e-14 {
        return None;
    }
    let mut inv = [[0.0; 3]; 3];
    inv[0][0] = (a[1][1] * a[2][2] - a[1][2] * a[2][1]) / det;
    inv[0][1] = (a[0][2] * a[2][1] - a[0][1] * a[2][2]) / det;
    inv[0][2] = (a[0][1] * a[1][2] - a[0][2] * a[1][1]) / det;
    inv[1][0] = (a[1][2] * a[2][0] - a[1][0] * a[2][2]) / det;
    inv[1][1] = (a[0][0] * a[2][2] - a[0][2] * a[2][0]) / det;
    inv[1][2] = (a[0][2] * a[1][0] - a[0][0] * a[1][2]) / det;
    inv[2][0] = (a[1][0] * a[2][1] - a[1][1] * a[2][0]) / det;
    inv[2][1] = (a[0][1] * a[2][0] - a[0][0] * a[2][1]) / det;
    inv[2][2] = (a[0][0] * a[1][1] - a[0][1] * a[1][0]) / det;
    Some(inv)
}

fn approx_rule2(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    if xs.is_empty() {
        return f64::NAN;
    }
    if x <= xs[0] {
        return ys[0];
    }
    let last = xs.len() - 1;
    if x >= xs[last] {
        return ys[last];
    }
    for i in 0..last {
        if x <= xs[i + 1] {
            let t = (x - xs[i]) / (xs[i + 1] - xs[i]);
            return ys[i] + t * (ys[i + 1] - ys[i]);
        }
    }
    ys[last]
}

/// GNU `PP.test(x)` Phillips-Perron unit-root test.
pub unsafe fn do_pp_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || XLENGTH(x) < 4 {
            return R_NilValue();
        }
        let n0 = XLENGTH(x) as usize;
        let mut xs = Vec::with_capacity(n0);
        for i in 0..n0 {
            xs.push(elt_real_safe(x, i as i64));
        }
        let n = n0 - 1;
        let mut xtx = [[0.0; 3]; 3];
        let mut xty = [0.0; 3];
        let mut yt1s = Vec::with_capacity(n);
        let mut yts = Vec::with_capacity(n);
        for i in 0..n {
            let yt = xs[i + 1];
            let yt1 = xs[i];
            let u = (i as f64 + 1.0) - (n as f64) / 2.0;
            let row = [1.0, u, yt1];
            yts.push(yt);
            yt1s.push(yt1);
            for a in 0..3 {
                xty[a] += row[a] * yt;
                for b in 0..3 {
                    xtx[a][b] += row[a] * row[b];
                }
            }
        }
        let Some(inv) = invert3(xtx) else {
            return R_NilValue();
        };
        let mut beta = [0.0; 3];
        for i in 0..3 {
            beta[i] = inv[i][0] * xty[0] + inv[i][1] * xty[1] + inv[i][2] * xty[2];
        }
        let mut resid = vec![0.0; n];
        let mut sse = 0.0;
        for i in 0..n {
            let u = (i as f64 + 1.0) - (n as f64) / 2.0;
            let fit = beta[0] + beta[1] * u + beta[2] * yt1s[i];
            let e = yts[i] - fit;
            resid[i] = e;
            sse += e * e;
        }
        let sigma2 = sse / ((n - 3) as f64);
        let se = (sigma2 * inv[2][2]).sqrt();
        let tstat = (beta[2] - 1.0) / se;
        let ssqru = sse / (n as f64);
        let l = (4.0 * (n as f64 / 100.0).powf(0.25)).trunc() as i32;
        let ssqrtl = ssqru + crate::library::stats::ppsum::r_pp_sum(&resid, l);
        let n_f = n as f64;
        let n2 = n_f * n_f;
        let mut sum_yt1_2 = 0.0;
        let mut sum_yt1_t = 0.0;
        let mut sum_yt1 = 0.0;
        for i in 0..n {
            let t = (i as f64) + 1.0;
            sum_yt1_2 += yt1s[i] * yt1s[i];
            sum_yt1_t += yt1s[i] * t;
            sum_yt1 += yt1s[i];
        }
        let trm1 = n2 * (n2 - 1.0) * sum_yt1_2 / 12.0;
        let trm2 = n_f * sum_yt1_t * sum_yt1_t;
        let trm3 = n_f * (n_f + 1.0) * sum_yt1_t * sum_yt1;
        let trm4 = (n_f * (n_f + 1.0) * (2.0 * n_f + 1.0) * sum_yt1 * sum_yt1) / 6.0;
        let dx = trm1 - trm2 + trm3 - trm4;
        let stat = ssqru.sqrt() / ssqrtl.sqrt() * tstat
            - (n_f * n_f * n_f) / (4.0 * 3.0_f64.sqrt() * dx.sqrt() * ssqrtl.sqrt())
                * (ssqrtl - ssqru);
        let table_t = [25.0, 50.0, 100.0, 250.0, 500.0, 1e5];
        let table = [
            [4.38, 4.15, 4.04, 3.99, 3.98, 3.96],
            [3.95, 3.80, 3.73, 3.69, 3.68, 3.66],
            [3.60, 3.50, 3.45, 3.43, 3.42, 3.41],
            [3.24, 3.18, 3.15, 3.13, 3.13, 3.12],
            [1.14, 1.19, 1.22, 1.23, 1.24, 1.25],
            [0.80, 0.87, 0.90, 0.92, 0.93, 0.94],
            [0.50, 0.58, 0.62, 0.64, 0.65, 0.66],
            [0.15, 0.24, 0.28, 0.31, 0.32, 0.33],
        ];
        let tablep = [0.01, 0.025, 0.05, 0.1, 0.9, 0.95, 0.975, 0.99];
        let mut tableipl = [0.0; 8];
        for i in 0..8 {
            let col: Vec<f64> = table[i].iter().map(|v| -v).collect();
            tableipl[i] = approx_rule2(&table_t, &col, n_f);
        }
        let pval = approx_rule2(&tableipl, &tablep, stat);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Dickey-Fuller".to_string()]);
        let parameter = Rf_ScalarReal(l as f64);
        let _pa = protect(parameter);
        set_string_names(parameter, &["Truncation lag parameter".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(
            result,
            3,
            Rf_mkString(c"Phillips-Perron Unit Root Test".as_ptr()),
        );
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn matmul_p2(x: &[f64], tt: [[f64; 2]; 2], p: usize) -> Vec<f64> {
    let mut z = vec![0.0; p * 2];
    for i in 0..p {
        let a = x[i];
        let b = x[i + p];
        z[i] = a * tt[0][0] + b * tt[1][0];
        z[i + p] = a * tt[0][1] + b * tt[1][1];
    }
    z
}

fn svd2_uvt(b00: f64, b10: f64, b01: f64, b11: f64) -> ([[f64; 2]; 2], f64) {
    // B is 2x2 column-major: [b00, b10; b01, b11]
    let a = b00 * b00 + b10 * b10;
    let c = b00 * b01 + b10 * b11;
    let bb = b01 * b01 + b11 * b11;
    let disc = ((a - bb) * (a - bb) + 4.0 * c * c).sqrt();
    let l1 = 0.5 * (a + bb + disc);
    let l2 = 0.5 * (a + bb - disc);
    let (v00, v10) = if c.abs() > 1e-18 {
        let mut x = c;
        let mut y = l1 - a;
        let n = (x * x + y * y).sqrt();
        x /= n;
        y /= n;
        (x, y)
    } else if a >= bb {
        (1.0, 0.0)
    } else {
        (0.0, 1.0)
    };
    let v01 = -v10;
    let v11 = v00;
    let s1 = l1.max(0.0).sqrt();
    let s2 = l2.max(0.0).sqrt();
    let mut u00 = b00 * v00 + b01 * v10;
    let mut u10 = b10 * v00 + b11 * v10;
    let mut u01 = b00 * v01 + b01 * v11;
    let mut u11 = b10 * v01 + b11 * v11;
    if s1 > 1e-18 {
        u00 /= s1;
        u10 /= s1;
    }
    if s2 > 1e-18 {
        u01 /= s2;
        u11 /= s2;
    }
    // TT = U %*% V^T
    let t00 = u00 * v00 + u01 * v01;
    let t01 = u00 * v10 + u01 * v11;
    let t10 = u10 * v00 + u11 * v01;
    let t11 = u10 * v10 + u11 * v11;
    ([[t00, t01], [t10, t11]], s1 + s2)
}

fn varimax_2col(x: &[f64], p: usize) -> (Vec<f64>, [[f64; 2]; 2]) {
    let mut work = vec![0.0; p * 2];
    let mut sc = vec![1.0; p];
    for i in 0..p {
        let a = x[i];
        let b = x[i + p];
        sc[i] = (a * a + b * b).sqrt();
        if sc[i] > 0.0 {
            work[i] = a / sc[i];
            work[i + p] = b / sc[i];
        } else {
            work[i] = a;
            work[i + p] = b;
        }
    }
    let mut tt = [[1.0, 0.0], [0.0, 1.0]];
    let mut d = 0.0;
    let pf = p as f64;
    for _ in 0..1000 {
        let z = matmul_p2(&work, tt, p);
        let mut c1 = 0.0;
        let mut c2 = 0.0;
        for i in 0..p {
            c1 += z[i] * z[i];
            c2 += z[i + p] * z[i + p];
        }
        c1 /= pf;
        c2 /= pf;
        let mut b00 = 0.0;
        let mut b10 = 0.0;
        let mut b01 = 0.0;
        let mut b11 = 0.0;
        for i in 0..p {
            let z1 = z[i];
            let z2 = z[i + p];
            let w1 = z1 * z1 * z1 - z1 * c1;
            let w2 = z2 * z2 * z2 - z2 * c2;
            b00 += work[i] * w1;
            b10 += work[i + p] * w1;
            b01 += work[i] * w2;
            b11 += work[i + p] * w2;
        }
        let (new_tt, dn) = svd2_uvt(b00, b10, b01, b11);
        let dpast = d;
        d = dn;
        tt = new_tt;
        if d < dpast * (1.0 + 1e-5) {
            break;
        }
    }
    let mut z = matmul_p2(&work, tt, p);
    for i in 0..p {
        z[i] *= sc[i];
        z[i + p] *= sc[i];
    }
    (z, tt)
}

/// GNU 2-column `varimax(x)`.
pub unsafe fn do_varimax(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (p, nc) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            return R_NilValue();
        };
        if nc != 2 || p < 2 {
            return R_NilValue();
        }
        let mut raw = vec![0.0; p * 2];
        for i in 0..(p * 2) {
            raw[i] = elt_real_safe(x, i as i64);
        }
        let (z, tt) = varimax_2col(&raw, p);
        let loadings =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), p as i32, 2);
        let _ld = protect(loadings);
        for i in 0..(p * 2) {
            *REAL(loadings).add(i) = z[i];
        }
        let class = Rf_mkString(c"loadings".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            loadings,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        let rot = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 2, 2);
        let _rt = protect(rot);
        *REAL(rot) = tt[0][0];
        *REAL(rot).add(1) = tt[1][0];
        *REAL(rot).add(2) = tt[0][1];
        *REAL(rot).add(3) = tt[1][1];
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, loadings);
        SET_VECTOR_ELT(result, 1, rot);
        crate::mainutils::essentials::set_string_names(
            result,
            &["loadings".to_string(), "rotmat".to_string()],
        );
        result
    }
}

fn invert2(a00: f64, a10: f64, a01: f64, a11: f64) -> Option<(f64, f64, f64, f64)> {
    let det = a00 * a11 - a01 * a10;
    if !det.is_finite() || det.abs() < 1e-18 {
        return None;
    }
    Some((a11 / det, -a10 / det, -a01 / det, a00 / det))
}

/// GNU 2-column `promax(x, m=4)`.
pub unsafe fn do_promax(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (p, nc) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            return R_NilValue();
        };
        if nc != 2 || p < 2 {
            return R_NilValue();
        }
        let mut raw = vec![0.0; p * 2];
        for i in 0..(p * 2) {
            raw[i] = elt_real_safe(x, i as i64);
        }
        let (vx, vrot) = varimax_2col(&raw, p);
        // Q = x * |x|^3  (m=4)
        let mut q = vec![0.0; p * 2];
        for i in 0..(p * 2) {
            q[i] = vx[i] * vx[i].abs().powi(3);
        }
        // U = (X'X)^{-1} X'Q
        let mut xtx = [0.0; 4];
        let mut xtq = [0.0; 4];
        for i in 0..p {
            let x1 = vx[i];
            let x2 = vx[i + p];
            let q1 = q[i];
            let q2 = q[i + p];
            xtx[0] += x1 * x1;
            xtx[1] += x2 * x1;
            xtx[2] += x1 * x2;
            xtx[3] += x2 * x2;
            xtq[0] += x1 * q1;
            xtq[1] += x2 * q1;
            xtq[2] += x1 * q2;
            xtq[3] += x2 * q2;
        }
        let Some(inv) = invert2(xtx[0], xtx[1], xtx[2], xtx[3]) else {
            return R_NilValue();
        };
        // U column-major
        let u00 = inv.0 * xtq[0] + inv.2 * xtq[1];
        let u10 = inv.1 * xtq[0] + inv.3 * xtq[1];
        let u01 = inv.0 * xtq[2] + inv.2 * xtq[3];
        let u11 = inv.1 * xtq[2] + inv.3 * xtq[3];
        // d = diag(solve(t(U) U))
        let tuu00 = u00 * u00 + u10 * u10;
        let tuu10 = u00 * u01 + u10 * u11;
        let tuu01 = tuu10;
        let tuu11 = u01 * u01 + u11 * u11;
        let Some(itu) = invert2(tuu00, tuu10, tuu01, tuu11) else {
            return R_NilValue();
        };
        let s0 = itu.0.max(0.0).sqrt();
        let s1 = itu.3.max(0.0).sqrt();
        let u00s = u00 * s0;
        let u10s = u10 * s0;
        let u01s = u01 * s1;
        let u11s = u11 * s1;
        let z = matmul_p2(&vx, [[u00s, u01s], [u10s, u11s]], p);
        let r00 = vrot[0][0] * u00s + vrot[0][1] * u10s;
        let r01 = vrot[0][0] * u01s + vrot[0][1] * u11s;
        let r10 = vrot[1][0] * u00s + vrot[1][1] * u10s;
        let r11 = vrot[1][0] * u01s + vrot[1][1] * u11s;
        let loadings =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), p as i32, 2);
        let _ld = protect(loadings);
        for i in 0..(p * 2) {
            *REAL(loadings).add(i) = z[i];
        }
        let class = Rf_mkString(c"loadings".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            loadings,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        let rot = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 2, 2);
        let _rt = protect(rot);
        *REAL(rot) = r00;
        *REAL(rot).add(1) = r10;
        *REAL(rot).add(2) = r01;
        *REAL(rot).add(3) = r11;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, loadings);
        SET_VECTOR_ELT(result, 1, rot);
        crate::mainutils::essentials::set_string_names(
            result,
            &["loadings".to_string(), "rotmat".to_string()],
        );
        result
    }
}

/// GNU `loadings(x)` — extract `$loadings`.
pub unsafe fn do_loadings(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        for i in 0..XLENGTH(names) {
            let s = STRING_ELT(names, i);
            if s.is_null() {
                continue;
            }
            let raw = CHAR(s);
            if raw.is_null() {
                continue;
            }
            if std::ffi::CStr::from_ptr(raw).to_string_lossy() == "loadings" {
                return VECTOR_ELT(x, i);
            }
        }
        R_NilValue()
    }
}

/// GNU `reorder(x, X)` by group means.
pub unsafe fn do_reorder(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let xx = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || xx.is_null() || xx == R_NilValue() {
            return R_NilValue();
        }
        let levels =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_LevelsSymbol());
        if levels.is_null() || levels == R_NilValue() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        let nlev = XLENGTH(levels) as usize;
        let n = XLENGTH(x) as usize;
        let mut codes = vec![0i32; n];
        for i in 0..n {
            codes[i] = if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(x).add(i)
            } else {
                elt_real_safe(x, i as i64).round() as i32
            };
        }
        let mut sums = vec![0.0; nlev];
        let mut cnt = vec![0.0; nlev];
        for i in 0..n {
            let g = codes[i];
            if g >= 1 && (g as usize) <= nlev {
                sums[(g as usize) - 1] += elt_real_safe(xx, i as i64);
                cnt[(g as usize) - 1] += 1.0;
            }
        }
        let mut scores = vec![0.0; nlev];
        let mut order: Vec<usize> = (0..nlev).collect();
        for i in 0..nlev {
            scores[i] = if cnt[i] > 0.0 {
                sums[i] / cnt[i]
            } else {
                f64::NAN
            };
        }
        order.sort_by(|&a, &b| {
            scores[a]
                .partial_cmp(&scores[b])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut new_code = vec![0i32; nlev];
        for (new_i, &old) in order.iter().enumerate() {
            new_code[old] = (new_i as i32) + 1;
        }
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n as i64);
        let _r = protect(result);
        for i in 0..n {
            let g = codes[i];
            *INTEGER(result).add(i) = if g >= 1 && (g as usize) <= nlev {
                new_code[(g as usize) - 1]
            } else {
                NA_INTEGER
            };
        }
        let new_levels = Rf_allocVector3(SEXPTYPE::STRSXP, nlev as i64);
        let _nl = protect(new_levels);
        for (new_i, &old) in order.iter().enumerate() {
            SET_STRING_ELT(new_levels, new_i as i64, STRING_ELT(levels, old as i64));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_LevelsSymbol(),
            new_levels,
        );
        let class = Rf_mkString(c"factor".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        let sc = Rf_allocVector3(SEXPTYPE::REALSXP, nlev as i64);
        let _sc = protect(sc);
        for i in 0..nlev {
            *REAL(sc).add(i) = scores[i];
        }
        crate::sexp::attrib_core::setAttrib(sc, crate::sexp::attrib_core::R_NamesSymbol(), levels);
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"scores".as_ptr()), sc);
        result
    }
}

/// GNU `oneway.test(x ~ g)` / `oneway.test(x, g)`.
pub unsafe fn do_oneway_test(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let first = CAR(args);
        let mut x = first;
        let mut g = CAR(CDR(args));
        let mut var_equal = false;
        if !first.is_null() && first != R_NilValue() && TYPEOF(first) == SEXPTYPE::LANGSXP {
            let lhs = CADR(first);
            let rhs = CAR(CDR(CDR(first)));
            if !lhs.is_null() && lhs != R_NilValue() {
                x = crate::eval::eval::Rf_eval(lhs, rho);
            }
            if !rhs.is_null() && rhs != R_NilValue() {
                g = crate::eval::eval::Rf_eval(rhs, rho);
            }
        }
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "var.equal" {
                let v = CAR(cell);
                if TYPEOF(v) == SEXPTYPE::LGLSXP && XLENGTH(v) > 0 {
                    var_equal = *LOGICAL(v) == TRUE;
                } else if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                    var_equal = *INTEGER(v) != 0;
                }
            }
            cell = CDR(cell);
        }
        if x.is_null() || x == R_NilValue() || g.is_null() || g == R_NilValue() {
            return R_NilValue();
        }
        let n0 = XLENGTH(x).min(XLENGTH(g));
        let mut pairs: Vec<(i32, f64)> = Vec::new();
        for i in 0..n0 {
            let gi = elt_real_safe(g, i);
            let xi = elt_real_safe(x, i);
            if gi.is_finite() && xi.is_finite() {
                pairs.push((gi.round() as i32, xi));
            }
        }
        let mut levels: Vec<i32> = pairs.iter().map(|p| p.0).collect();
        levels.sort_unstable();
        levels.dedup();
        let k = levels.len();
        if k < 2 {
            return R_NilValue();
        }
        let mut ns = vec![0.0; k];
        let mut means = vec![0.0; k];
        let mut vars = vec![0.0; k];
        let mut all = Vec::new();
        for (li, &lev) in levels.iter().enumerate() {
            let vs: Vec<f64> = pairs.iter().filter(|p| p.0 == lev).map(|p| p.1).collect();
            let n = vs.len() as f64;
            ns[li] = n;
            let m = vs.iter().sum::<f64>() / n;
            means[li] = m;
            vars[li] = vs.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (n - 1.0);
            all.extend(vs);
        }
        let (stat, df1, df2, method) = if var_equal {
            let n = all.len() as f64;
            let grand = all.iter().sum::<f64>() / n;
            let ssb: f64 = ns
                .iter()
                .zip(means.iter())
                .map(|(ni, mi)| ni * (mi - grand) * (mi - grand))
                .sum();
            let ssw: f64 = ns
                .iter()
                .zip(vars.iter())
                .map(|(ni, vi)| (ni - 1.0) * vi)
                .sum();
            let df1 = (k - 1) as f64;
            let df2 = n - k as f64;
            let stat = (ssb / df1) / (ssw / df2);
            (stat, df1, df2, "One-way analysis of means")
        } else {
            let w: Vec<f64> = ns.iter().zip(vars.iter()).map(|(n, v)| n / v).collect();
            let sum_w: f64 = w.iter().sum();
            let m = w
                .iter()
                .zip(means.iter())
                .map(|(wi, mi)| wi * mi)
                .sum::<f64>()
                / sum_w;
            let tmp: f64 = w
                .iter()
                .zip(ns.iter())
                .map(|(wi, ni)| {
                    let t = 1.0 - wi / sum_w;
                    t * t / (ni - 1.0)
                })
                .sum::<f64>()
                / ((k * k - 1) as f64);
            let stat = w
                .iter()
                .zip(means.iter())
                .map(|(wi, mi)| wi * (mi - m) * (mi - m))
                .sum::<f64>()
                / ((k as f64 - 1.0) * (1.0 + 2.0 * (k as f64 - 2.0) * tmp));
            let df1 = (k - 1) as f64;
            let df2 = 1.0 / (3.0 * tmp);
            (
                stat,
                df1,
                df2,
                "One-way analysis of means (not assuming equal variances)",
            )
        };
        let pval = crate::dist::f_dist::pf_inner(stat, df1, df2, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["F".to_string()]);
        let parameter = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _pa = protect(parameter);
        *REAL(parameter) = df1;
        *REAL(parameter).add(1) = df2;
        set_string_names(parameter, &["num df".to_string(), "denom df".to_string()]);
        let method_s = if var_equal {
            Rf_mkString(c"One-way analysis of means".as_ptr())
        } else {
            Rf_mkString(c"One-way analysis of means (not assuming equal variances)".as_ptr())
        };
        let _ = method;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, method_s);
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x and g".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn loglik_numeric(x: SEXP) -> f64 {
    unsafe { elt_real_safe(x, 0) }
}

fn loglik_df(x: SEXP) -> f64 {
    unsafe {
        let df = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"df".as_ptr()));
        if !df.is_null() && df != R_NilValue() {
            elt_real_safe(df, 0)
        } else {
            0.0
        }
    }
}

fn loglik_nobs(x: SEXP) -> f64 {
    unsafe {
        let n = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"nobs".as_ptr()));
        if !n.is_null() && n != R_NilValue() {
            elt_real_safe(n, 0)
        } else {
            0.0
        }
    }
}

/// GNU `AIC(object, k=2)` for a `logLik`.
pub unsafe fn do_aic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mut k = 2.0;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "k" || name.is_empty() {
                let v = CAR(cell);
                if !v.is_null() && v != R_NilValue() {
                    let kv = elt_real_safe(v, 0);
                    if kv.is_finite() {
                        k = kv;
                    }
                }
            }
            cell = CDR(cell);
        }
        Rf_ScalarReal(-2.0 * loglik_numeric(x) + k * loglik_df(x))
    }
}

/// GNU `BIC(object)` for a `logLik`.
pub unsafe fn do_bic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = loglik_nobs(x);
        Rf_ScalarReal(-2.0 * loglik_numeric(x) + loglik_df(x) * n.ln())
    }
}

/// GNU `logLik` identity for an already-tagged logLik.
pub unsafe fn do_loglik(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { CAR(args) }
}

unsafe fn list_named_elt(list: SEXP, name: &str) -> SEXP {
    unsafe {
        if list.is_null() || list == R_NilValue() || TYPEOF(list) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names =
            crate::sexp::attrib_core::getAttrib(list, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        for i in 0..XLENGTH(names) {
            let s = STRING_ELT(names, i);
            if s.is_null() {
                continue;
            }
            let raw = CHAR(s);
            if raw.is_null() {
                continue;
            }
            if std::ffi::CStr::from_ptr(raw).to_string_lossy() == name {
                return VECTOR_ELT(list, i);
            }
        }
        R_NilValue()
    }
}

/// GNU `sigma(object)` for a list with deviance, nobs, coefficients.
pub unsafe fn do_sigma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dev = list_named_elt(x, "deviance");
        let nobs = list_named_elt(x, "nobs");
        let coef = list_named_elt(x, "coefficients");
        if dev == R_NilValue() || nobs == R_NilValue() {
            return R_NilValue();
        }
        let d = elt_real_safe(dev, 0);
        let n = elt_real_safe(nobs, 0);
        let p = if coef == R_NilValue() {
            0.0
        } else {
            XLENGTH(coef) as f64
        };
        let den = n - p;
        if den <= 0.0 {
            return Rf_ScalarReal(f64::NAN);
        }
        Rf_ScalarReal((d / den).sqrt())
    }
}

/// GNU `extractAIC(fit, k)` glm-style: `c(edf, aic + (k-2)*edf)`.
pub unsafe fn do_extract_aic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fit = CAR(args);
        let mut k = 2.0;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "k" {
                let kv = elt_real_safe(CAR(cell), 0);
                if kv.is_finite() {
                    k = kv;
                }
            }
            cell = CDR(cell);
        }
        let resid = list_named_elt(fit, "residuals");
        let dfr = list_named_elt(fit, "df.residual");
        let aic = list_named_elt(fit, "aic");
        if resid == R_NilValue() || dfr == R_NilValue() || aic == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid) as f64;
        let edf = n - elt_real_safe(dfr, 0);
        let a = elt_real_safe(aic, 0);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _r = protect(result);
        *REAL(result) = edf;
        *REAL(result).add(1) = a + (k - 2.0) * edf;
        result
    }
}

/// GNU `case.names(object)` — rownames.
pub unsafe fn do_case_names(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::essentials::do_rownames(call, op, args, rho) }
}

/// GNU `variable.names(object)` — colnames.
pub unsafe fn do_variable_names(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::essentials::do_colnames(call, op, args, rho) }
}

/// GNU `confint(object)` from `$coefficients` and `$vcov`.
pub unsafe fn do_confint(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let mut level = 0.95;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "level" {
                let v = elt_real_safe(CAR(cell), 0);
                if v.is_finite() && v > 0.0 && v < 1.0 {
                    level = v;
                }
            }
            cell = CDR(cell);
        }
        let cf = list_named_elt(obj, "coefficients");
        let vcov = list_named_elt(obj, "vcov");
        if cf == R_NilValue() || vcov == R_NilValue() {
            return R_NilValue();
        }
        let p = XLENGTH(cf) as usize;
        if p == 0 {
            return R_NilValue();
        }
        let a = (1.0 - level) / 2.0;
        let zlo = crate::dist::normal::qnorm5_inner(a, 0.0, 1.0, true, false);
        let zhi = crate::dist::normal::qnorm5_inner(1.0 - a, 0.0, 1.0, true, false);
        let result =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), p as i32, 2);
        let _r = protect(result);
        for i in 0..p {
            let est = elt_real_safe(cf, i as i64);
            let var = elt_real_safe(vcov, (i + i * p) as i64);
            let se = var.max(0.0).sqrt();
            *REAL(result).add(i) = est + se * zlo;
            *REAL(result).add(i + p) = est + se * zhi;
        }
        let rn = crate::sexp::attrib_core::getAttrib(cf, crate::sexp::attrib_core::R_NamesSymbol());
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _cn = protect(cn);
        let lo_pct = format!("{} %", (100.0 * a * 10.0).round() / 10.0);
        let hi_pct = format!("{} %", (100.0 * (1.0 - a) * 10.0).round() / 10.0);
        let lo_c = std::ffi::CString::new(lo_pct).unwrap_or_default();
        let hi_c = std::ffi::CString::new(hi_pct).unwrap_or_default();
        SET_STRING_ELT(cn, 0, Rf_mkChar(lo_c.as_ptr()));
        SET_STRING_ELT(cn, 1, Rf_mkChar(hi_c.as_ptr()));
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, if rn != R_NilValue() { rn } else { R_NilValue() });
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dn,
        );
        result
    }
}

/// GNU `vcov(object)` — extract `$vcov`.
pub unsafe fn do_vcov(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let v = list_named_elt(CAR(args), "vcov");
        if v == R_NilValue() { R_NilValue() } else { v }
    }
}

/// GNU `hat(x)` leverages for intercept + x.
pub unsafe fn do_hat(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x) as usize;
        if n < 2 {
            return R_NilValue();
        }
        let mut xs = Vec::with_capacity(n);
        for i in 0..n {
            xs.push(elt_real_safe(x, i as i64));
        }
        let mean = xs.iter().sum::<f64>() / n as f64;
        let sxx = xs.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>();
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _r = protect(result);
        let invn = 1.0 / n as f64;
        for i in 0..n {
            let h = if sxx > 0.0 {
                invn + (xs[i] - mean) * (xs[i] - mean) / sxx
            } else {
                invn
            };
            *REAL(result).add(i) = h;
        }
        result
    }
}

/// GNU `hatvalues(model)` — extract `$hat` or compute `hat(residuals)`.
pub unsafe fn do_hatvalues(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let h = list_named_elt(obj, "hat");
        if h != R_NilValue() {
            return h;
        }
        let x = list_named_elt(obj, "x");
        if x != R_NilValue() {
            return do_hat(_call, _op, Rf_cons(x, R_NilValue()), rho);
        }
        R_NilValue()
    }
}

/// GNU `rstandard(model)` as `resid / (sigma * sqrt(1-hat))`.
pub unsafe fn do_rstandard(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let resid = list_named_elt(obj, "residuals");
        let hat = list_named_elt(obj, "hat");
        let sigma = list_named_elt(obj, "sigma");
        if resid == R_NilValue() || hat == R_NilValue() || sigma == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid).min(XLENGTH(hat));
        let s = elt_real_safe(sigma, 0);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let e = elt_real_safe(resid, i);
            let h = elt_real_safe(hat, i);
            let den = s * (1.0 - h).max(0.0).sqrt();
            *REAL(result).add(i as usize) = if den > 0.0 && den.is_finite() {
                e / den
            } else {
                f64::NAN
            };
        }
        result
    }
}

/// GNU `cooks.distance` as `((e/((1-h)*sd))^2 * h) / p`.
pub unsafe fn do_cooks_distance(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let resid = list_named_elt(obj, "residuals");
        let hat = list_named_elt(obj, "hat");
        let sigma = list_named_elt(obj, "sigma");
        let rank = list_named_elt(obj, "rank");
        if resid == R_NilValue() || hat == R_NilValue() || sigma == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid).min(XLENGTH(hat));
        let s = elt_real_safe(sigma, 0);
        let p = if rank == R_NilValue() {
            1.0
        } else {
            elt_real_safe(rank, 0)
        };
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let e = elt_real_safe(resid, i);
            let h = elt_real_safe(hat, i);
            let den = (1.0 - h) * s;
            let d = if den != 0.0 && p != 0.0 && den.is_finite() {
                let t = e / den;
                t * t * h / p
            } else {
                f64::NAN
            };
            *REAL(result).add(i as usize) = d;
        }
        result
    }
}

/// GNU `dffits` as `e * sqrt(h) / (sigma * (1-h))`.
pub unsafe fn do_dffits(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let resid = list_named_elt(obj, "residuals");
        let hat = list_named_elt(obj, "hat");
        let sigma = list_named_elt(obj, "sigma");
        if resid == R_NilValue() || hat == R_NilValue() || sigma == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid).min(XLENGTH(hat));
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let e = elt_real_safe(resid, i);
            let h = elt_real_safe(hat, i);
            let s = if XLENGTH(sigma) > 1 {
                elt_real_safe(sigma, i)
            } else {
                elt_real_safe(sigma, 0)
            };
            let den = s * (1.0 - h);
            *REAL(result).add(i as usize) = if den != 0.0 && den.is_finite() && h >= 0.0 {
                e * h.sqrt() / den
            } else {
                f64::NAN
            };
        }
        result
    }
}

/// GNU `rstudent` as `e / (sigma_i * sqrt(1-hat))`.
pub unsafe fn do_rstudent(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let resid = list_named_elt(obj, "residuals");
        let hat = list_named_elt(obj, "hat");
        let sigma = list_named_elt(obj, "sigma");
        if resid == R_NilValue() || hat == R_NilValue() || sigma == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid).min(XLENGTH(hat));
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let e = elt_real_safe(resid, i);
            let h = elt_real_safe(hat, i);
            let s = if XLENGTH(sigma) > 1 {
                elt_real_safe(sigma, i)
            } else {
                elt_real_safe(sigma, 0)
            };
            let den = s * (1.0 - h).max(0.0).sqrt();
            *REAL(result).add(i as usize) = if den > 0.0 && den.is_finite() {
                e / den
            } else {
                f64::NAN
            };
        }
        result
    }
}

/// GNU `lm(y ~ x)` intercept + slope.
pub unsafe fn do_lm(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let first = CAR(args);
        let mut y = first;
        let mut preds: Vec<(String, SEXP)> = Vec::new();
        if !first.is_null() && first != R_NilValue() && TYPEOF(first) == SEXPTYPE::LANGSXP {
            let lhs = CADR(first);
            let rhs = CAR(CDR(CDR(first)));
            if !lhs.is_null() && lhs != R_NilValue() {
                y = crate::eval::eval::Rf_eval(lhs, rho);
            }
            fn collect_plus(expr: SEXP, out: &mut Vec<SEXP>) {
                unsafe {
                    if expr.is_null() || expr == R_NilValue() {
                        return;
                    }
                    if TYPEOF(expr) == SEXPTYPE::LANGSXP {
                        let op = CAR(expr);
                        let name = if !op.is_null() && TYPEOF(op) == SEXPTYPE::SYMSXP {
                            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(op)))
                                .to_string_lossy()
                                .into_owned()
                        } else {
                            String::new()
                        };
                        if name == "+" {
                            collect_plus(CADR(expr), out);
                            collect_plus(CAR(CDR(CDR(expr))), out);
                            return;
                        }
                    }
                    out.push(expr);
                }
            }
            let mut terms = Vec::new();
            collect_plus(rhs, &mut terms);
            for term in terms {
                let name = if TYPEOF(term) == SEXPTYPE::SYMSXP {
                    std::ffi::CStr::from_ptr(CHAR(PRINTNAME(term)))
                        .to_string_lossy()
                        .into_owned()
                } else {
                    "x".to_string()
                };
                let val = crate::eval::eval::Rf_eval(term, rho);
                preds.push((name, val));
            }
        } else {
            let x = CAR(CDR(args));
            preds.push(("x".to_string(), x));
        }
        if y.is_null() || y == R_NilValue() || preds.is_empty() {
            return R_NilValue();
        }
        if preds.len() >= 2 {
            let x1 = preds[0].1;
            let x2 = preds[1].1;
            if x1.is_null() || x1 == R_NilValue() || x2.is_null() || x2 == R_NilValue() {
                return R_NilValue();
            }
            let n = XLENGTH(y).min(XLENGTH(x1)).min(XLENGTH(x2)) as usize;
            if n < 3 {
                return R_NilValue();
            }
            let mut ys = vec![0.0; n];
            let mut a = vec![0.0; n];
            let mut b = vec![0.0; n];
            for i in 0..n {
                ys[i] = elt_real_safe(y, i as i64);
                a[i] = elt_real_safe(x1, i as i64);
                b[i] = elt_real_safe(x2, i as i64);
            }
            let mut xtx = [[0.0; 3]; 3];
            let mut xty = [0.0; 3];
            for i in 0..n {
                let row = [1.0, a[i], b[i]];
                for p in 0..3 {
                    xty[p] += row[p] * ys[i];
                    for q in 0..3 {
                        xtx[p][q] += row[p] * row[q];
                    }
                }
            }
            let Some(inv) = invert3(xtx) else {
                return R_NilValue();
            };
            let mut beta = [0.0; 3];
            for i in 0..3 {
                beta[i] = inv[i][0] * xty[0] + inv[i][1] * xty[1] + inv[i][2] * xty[2];
            }
            let coef = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
            let _c = protect(coef);
            *REAL(coef) = beta[0];
            *REAL(coef).add(1) = beta[1];
            *REAL(coef).add(2) = beta[2];
            set_string_names(
                coef,
                &[
                    "(Intercept)".to_string(),
                    preds[0].0.clone(),
                    preds[1].0.clone(),
                ],
            );
            let fitted = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
            let _f = protect(fitted);
            let resid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
            let _e = protect(resid);
            let hats = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
            let _h = protect(hats);
            let mut sse = 0.0;
            for i in 0..n {
                let row = [1.0, a[i], b[i]];
                let fit = beta[0] * row[0] + beta[1] * row[1] + beta[2] * row[2];
                let e = ys[i] - fit;
                *REAL(fitted).add(i) = fit;
                *REAL(resid).add(i) = e;
                sse += e * e;
                let mut tmp = [0.0; 3];
                for p in 0..3 {
                    tmp[p] = inv[p][0] * row[0] + inv[p][1] * row[1] + inv[p][2] * row[2];
                }
                *REAL(hats).add(i) = row[0] * tmp[0] + row[1] * tmp[1] + row[2] * tmp[2];
            }
            let df = (n as i64) - 3;
            let sigma = if df > 0 {
                (sse / df as f64).sqrt()
            } else {
                f64::NAN
            };
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 7);
            let _r = protect(result);
            SET_VECTOR_ELT(result, 0, coef);
            SET_VECTOR_ELT(result, 1, resid);
            SET_VECTOR_ELT(result, 2, fitted);
            SET_VECTOR_ELT(result, 3, Rf_ScalarInteger(3));
            SET_VECTOR_ELT(result, 4, Rf_ScalarInteger(df as i32));
            SET_VECTOR_ELT(result, 5, Rf_ScalarReal(sigma));
            SET_VECTOR_ELT(result, 6, hats);
            crate::mainutils::essentials::set_string_names(
                result,
                &[
                    "coefficients".to_string(),
                    "residuals".to_string(),
                    "fitted.values".to_string(),
                    "rank".to_string(),
                    "df.residual".to_string(),
                    "sigma".to_string(),
                    "hat".to_string(),
                ],
            );
            let class = Rf_mkString(c"lm".as_ptr());
            let _cl = protect(class);


            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
            return result;
        }
        let x = preds[0].1;
        let xname = preds[0].0.clone();
        if y.is_null() || y == R_NilValue() || x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(y).min(XLENGTH(x)) as usize;
        if n < 2 {
            return R_NilValue();
        }
        let mut ys = Vec::with_capacity(n);
        let mut xs = Vec::with_capacity(n);
        for i in 0..n {
            ys.push(elt_real_safe(y, i as i64));
            xs.push(elt_real_safe(x, i as i64));
        }
        let mut sxx = 0.0;
        let mut sxy = 0.0;
        let mut sx = 0.0;
        let mut sy = 0.0;
        for i in 0..n {
            sx += xs[i];
            sy += ys[i];
            sxx += xs[i] * xs[i];
            sxy += xs[i] * ys[i];
        }
        let nf = n as f64;
        let Some(inv) = invert2(nf, sx, sx, sxx) else {
            return R_NilValue();
        };
        let b0 = inv.0 * sy + inv.2 * sxy;
        let b1 = inv.1 * sy + inv.3 * sxy;
        let coef = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _c = protect(coef);
        *REAL(coef) = b0;
        *REAL(coef).add(1) = b1;
        set_string_names(coef, &["(Intercept)".to_string(), xname]);
        let fitted = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _f = protect(fitted);
        let resid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _e = protect(resid);
        let mut sse = 0.0;
        for i in 0..n {
            let fit = b0 + b1 * xs[i];
            let e = ys[i] - fit;
            *REAL(fitted).add(i) = fit;
            *REAL(resid).add(i) = e;
            sse += e * e;
        }
        let df = (n as i64) - 2;
        let sigma = if df > 0 {
            (sse / df as f64).sqrt()
        } else {
            f64::NAN
        };
        let hats = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _h = protect(hats);
        let meanx = sx / nf;
        let sxxc = xs.iter().map(|v| (v - meanx) * (v - meanx)).sum::<f64>();
        for i in 0..n {
            let h = if sxxc > 0.0 {
                1.0 / nf + (xs[i] - meanx) * (xs[i] - meanx) / sxxc
            } else {
                1.0 / nf
            };
            *REAL(hats).add(i) = h;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 7);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, coef);
        SET_VECTOR_ELT(result, 1, resid);
        SET_VECTOR_ELT(result, 2, fitted);
        SET_VECTOR_ELT(result, 3, Rf_ScalarInteger(2));
        SET_VECTOR_ELT(result, 4, Rf_ScalarInteger(df as i32));
        SET_VECTOR_ELT(result, 5, Rf_ScalarReal(sigma));
        SET_VECTOR_ELT(result, 6, hats);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "coefficients".to_string(),
                "residuals".to_string(),
                "fitted.values".to_string(),
                "rank".to_string(),
                "df.residual".to_string(),
                "sigma".to_string(),
                "hat".to_string(),
            ],
        );
        let class = Rf_mkString(c"lm".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}
unsafe fn family_object(family: &str, link: &str) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        let fam = match family {
            "gaussian" => Rf_mkString(c"gaussian".as_ptr()),
            "poisson" => Rf_mkString(c"poisson".as_ptr()),
            "Gamma" => Rf_mkString(c"Gamma".as_ptr()),
            _ => Rf_mkString(c"binomial".as_ptr()),
        };
        let lnk = match link {
            "identity" => Rf_mkString(c"identity".as_ptr()),
            "probit" => Rf_mkString(c"probit".as_ptr()),
            "log" => Rf_mkString(c"log".as_ptr()),
            "inverse" => Rf_mkString(c"inverse".as_ptr()),
            _ => Rf_mkString(c"logit".as_ptr()),
        };
        SET_VECTOR_ELT(result, 0, fam);
        SET_VECTOR_ELT(result, 1, lnk);
        crate::mainutils::essentials::set_string_names(
            result,
            &["family".to_string(), "link".to_string()],
        );
        let class = Rf_mkString(c"family".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `binomial()` family object.
pub unsafe fn do_binomial(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut link = "logit".to_string();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                link = elt_to_string(v, 0);
            }
            cell = CDR(cell);
        }
        family_object("binomial", &link)
    }
}

/// GNU `gaussian()` family object.
pub unsafe fn do_gaussian(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut link = "identity".to_string();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                link = elt_to_string(v, 0);
            }
            cell = CDR(cell);
        }
        family_object("gaussian", &link)
    }
}

/// GNU `poisson()` family object.
pub unsafe fn do_poisson(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut link = "log".to_string();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                link = elt_to_string(v, 0);
            }
            cell = CDR(cell);
        }
        family_object("poisson", &link)
    }
}

/// GNU `Gamma()` family object.
pub unsafe fn do_gamma_family(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut link = "inverse".to_string();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                link = elt_to_string(v, 0);
            }
            cell = CDR(cell);
        }
        family_object("Gamma", &link)
    }
}



/// GNU `aov(y ~ x)` — `lm` with class `c("aov","lm")`.
pub unsafe fn do_aov(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let result = do_lm(call, op, args, rho);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _cl = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"aov".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"lm".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `glm(y ~ x)` gaussian — `lm` with class `c("glm","lm")`.
pub unsafe fn do_glm(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let result = do_lm(call, op, args, rho);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _cl = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"glm".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"lm".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `covratio(lm)`.
pub unsafe fn do_covratio(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let resid = list_named_elt(obj, "residuals");
        let hat = list_named_elt(obj, "hat");
        let sigma = list_named_elt(obj, "sigma");
        let rank = list_named_elt(obj, "rank");
        if resid == R_NilValue() || hat == R_NilValue() || sigma == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid).min(XLENGTH(hat));
        let p = if rank == R_NilValue() {
            2.0
        } else {
            elt_real_safe(rank, 0)
        };
        let s = elt_real_safe(sigma, 0);
        let sse = s * s * (n as f64 - p);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let e = elt_real_safe(resid, i);
            let h = elt_real_safe(hat, i);
            let omh = 1.0 - h;
            let infl_s2 = if omh > 0.0 && n as f64 - p - 1.0 > 0.0 {
                (sse - e * e / omh) / (n as f64 - p - 1.0)
            } else {
                f64::NAN
            };
            let infl_s = infl_s2.max(0.0).sqrt();
            let estar = if infl_s > 0.0 && omh > 0.0 {
                e / (infl_s * omh.sqrt())
            } else {
                f64::NAN
            };
            let inner = (n as f64 - p - 1.0 + estar * estar) / (n as f64 - p);
            *REAL(result).add(i as usize) = if omh > 0.0 && inner.is_finite() {
                1.0 / (omh * inner.powf(p))
            } else {
                f64::NAN
            };
        }
        result
    }
}

/// GNU `predict(lm)` and `predict(lm, newdata)`.
pub unsafe fn do_predict_lm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let coef = list_named_elt(obj, "coefficients");
        if coef == R_NilValue() || XLENGTH(coef) < 2 {
            return list_named_elt(obj, "fitted.values");
        }
        let b0 = elt_real_safe(coef, 0);
        let b1 = elt_real_safe(coef, 1);
        let mut newdata = R_NilValue();
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "newdata" || name.is_empty() {
                newdata = CAR(cell);
            }
            cell = CDR(cell);
        }
        if newdata.is_null() || newdata == R_NilValue() {
            return list_named_elt(obj, "fitted.values");
        }
        let ncoef = XLENGTH(coef);
        if ncoef >= 3 && TYPEOF(newdata) == SEXPTYPE::VECSXP {
            let names = crate::sexp::attrib_core::getAttrib(
                coef,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            let n1 = if TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) >= 3 {
                std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 1)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                "x1".to_string()
            };
            let n2 = if TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) >= 3 {
                std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 2)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                "x2".to_string()
            };
            let x1 = list_named_elt(newdata, &n1);
            let x2 = list_named_elt(newdata, &n2);
            if x1 != R_NilValue() && x2 != R_NilValue() {
                let b2 = elt_real_safe(coef, 2);
                let n = XLENGTH(x1).min(XLENGTH(x2));
                let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
                let _r = protect(result);
                for i in 0..n {
                    *REAL(result).add(i as usize) =
                        b0 + b1 * elt_real_safe(x1, i) + b2 * elt_real_safe(x2, i);
                }
                return result;
            }
        }
        let x = if TYPEOF(newdata) == SEXPTYPE::VECSXP {
            let named = list_named_elt(newdata, "x");
            if named != R_NilValue() {
                named
            } else if XLENGTH(newdata) > 0 {
                VECTOR_ELT(newdata, 0)
            } else {
                R_NilValue()
            }
        } else {
            newdata
        };
        if x == R_NilValue() {
            return list_named_elt(obj, "fitted.values");
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            *REAL(result).add(i as usize) = b0 + b1 * elt_real_safe(x, i);
        }
        result
    }
}

/// GNU `summary(lm)` coefficient table and fit stats.
pub unsafe fn do_summary_lm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let coef = list_named_elt(obj, "coefficients");
        let resid = list_named_elt(obj, "residuals");
        let fitted = list_named_elt(obj, "fitted.values");
        let sigma = list_named_elt(obj, "sigma");
        let dfr = list_named_elt(obj, "df.residual");
        if coef == R_NilValue() || resid == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid) as usize;
        let df = if dfr != R_NilValue() {
            elt_real_safe(dfr, 0)
        } else {
            (n as f64) - 2.0
        };
        let s = if sigma != R_NilValue() {
            elt_real_safe(sigma, 0)
        } else {
            0.0
        };
        let b0 = elt_real_safe(coef, 0);
        let b1 = elt_real_safe(coef, 1);
        let mut sse = 0.0;
        let mut sst = 0.0;
        let mut ysum = 0.0;
        let mut ys = Vec::with_capacity(n);
        let mut xs = Vec::with_capacity(n);
        for i in 0..n {
            let e = elt_real_safe(resid, i as i64);
            let f = if fitted != R_NilValue() {
                elt_real_safe(fitted, i as i64)
            } else {
                0.0
            };
            let y = f + e;
            ys.push(y);
            ysum += y;
            sse += e * e;
            let xi = if b1.abs() > 1e-15 {
                (f - b0) / b1
            } else {
                i as f64
            };
            xs.push(xi);
        }
        let ybar = ysum / n as f64;
        for y in &ys {
            sst += (y - ybar) * (y - ybar);
        }
        let meanx = xs.iter().sum::<f64>() / n as f64;
        let sxx = xs.iter().map(|v| (v - meanx) * (v - meanx)).sum::<f64>();
        let se1 = if sxx > 0.0 { s / sxx.sqrt() } else { f64::NAN };
        let se0 = if sxx > 0.0 {
            s * (1.0 / n as f64 + meanx * meanx / sxx).sqrt()
        } else {
            f64::NAN
        };
        let t0 = b0 / se0;
        let t1 = b1 / se1;
        let p0 = 2.0 * crate::dist::t_dist::pt_inner(-t0.abs(), df, true, false);
        let p1 = 2.0 * crate::dist::t_dist::pt_inner(-t1.abs(), df, true, false);
        let r2 = if sst > 0.0 { 1.0 - sse / sst } else { f64::NAN };
        let adj = if n > 2 {
            1.0 - (1.0 - r2) * ((n as f64 - 1.0) / df)
        } else {
            r2
        };
        let fstat = if sse > 0.0 && df > 0.0 {
            ((sst - sse) / 1.0) / (sse / df)
        } else {
            f64::NAN
        };
        let ctab = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 2, 4);
        let _ct = protect(ctab);
        *REAL(ctab) = b0;
        *REAL(ctab).add(1) = b1;
        *REAL(ctab).add(2) = se0;
        *REAL(ctab).add(3) = se1;
        *REAL(ctab).add(4) = t0;
        *REAL(ctab).add(5) = t1;
        *REAL(ctab).add(6) = p0;
        *REAL(ctab).add(7) = p1;
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _rn = protect(rn);
        SET_STRING_ELT(rn, 0, Rf_mkChar(c"(Intercept)".as_ptr()));
        SET_STRING_ELT(rn, 1, Rf_mkChar(c"x".as_ptr()));
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 4);
        let _cn = protect(cn);
        SET_STRING_ELT(cn, 0, Rf_mkChar(c"Estimate".as_ptr()));
        SET_STRING_ELT(cn, 1, Rf_mkChar(c"Std. Error".as_ptr()));
        SET_STRING_ELT(cn, 2, Rf_mkChar(c"t value".as_ptr()));
        SET_STRING_ELT(cn, 3, Rf_mkChar(c"Pr(>|t|)".as_ptr()));
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(ctab, crate::sexp::attrib_core::R_DimNamesSymbol(), dn);
        let fvec = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        let _fv = protect(fvec);
        *REAL(fvec) = fstat;
        *REAL(fvec).add(1) = 1.0;
        *REAL(fvec).add(2) = df;
        set_string_names(
            fvec,
            &[
                "value".to_string(),
                "numdf".to_string(),
                "dendf".to_string(),
            ],
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, ctab);
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(s));
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(r2));
        SET_VECTOR_ELT(result, 3, Rf_ScalarReal(adj));
        SET_VECTOR_ELT(result, 4, fvec);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "coefficients".to_string(),
                "sigma".to_string(),
                "r.squared".to_string(),
                "adj.r.squared".to_string(),
                "fstatistic".to_string(),
            ],
        );
        let class = Rf_mkString(c"summary.lm".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `anova(lm)` one-term table.
pub unsafe fn do_anova_lm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let resid = list_named_elt(obj, "residuals");
        let fitted = list_named_elt(obj, "fitted.values");
        let dfr = list_named_elt(obj, "df.residual");
        if resid == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid) as usize;
        let df_res = if dfr != R_NilValue() {
            elt_real_safe(dfr, 0)
        } else {
            (n as f64) - 2.0
        };
        let mut sse = 0.0;
        let mut ysum = 0.0;
        let mut ys = Vec::with_capacity(n);
        for i in 0..n {
            let e = elt_real_safe(resid, i as i64);
            let f = if fitted != R_NilValue() {
                elt_real_safe(fitted, i as i64)
            } else {
                0.0
            };
            let y = f + e;
            ys.push(y);
            ysum += y;
            sse += e * e;
        }
        let ybar = ysum / n as f64;
        let sst: f64 = ys.iter().map(|y| (y - ybar) * (y - ybar)).sum();
        let ssr = sst - sse;
        let df_mod = 1.0;
        let msr = ssr / df_mod;
        let mse = if df_res > 0.0 { sse / df_res } else { f64::NAN };
        let f = if mse > 0.0 { msr / mse } else { f64::NAN };
        let p = crate::dist::f_dist::pf_inner(f, df_mod, df_res, false, false);
        let tab = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 2, 5);
        let _t = protect(tab);
        *REAL(tab) = df_mod;
        *REAL(tab).add(1) = df_res;
        *REAL(tab).add(2) = ssr;
        *REAL(tab).add(3) = sse;
        *REAL(tab).add(4) = msr;
        *REAL(tab).add(5) = mse;
        *REAL(tab).add(6) = f;
        *REAL(tab).add(7) = NA_REAL;
        *REAL(tab).add(8) = p;
        *REAL(tab).add(9) = NA_REAL;
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _rn = protect(rn);
        SET_STRING_ELT(rn, 0, Rf_mkChar(c"x".as_ptr()));
        SET_STRING_ELT(rn, 1, Rf_mkChar(c"Residuals".as_ptr()));
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 5);
        let _cn = protect(cn);
        SET_STRING_ELT(cn, 0, Rf_mkChar(c"Df".as_ptr()));
        SET_STRING_ELT(cn, 1, Rf_mkChar(c"Sum Sq".as_ptr()));
        SET_STRING_ELT(cn, 2, Rf_mkChar(c"Mean Sq".as_ptr()));
        SET_STRING_ELT(cn, 3, Rf_mkChar(c"F value".as_ptr()));
        SET_STRING_ELT(cn, 4, Rf_mkChar(c"Pr(>F)".as_ptr()));
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(tab, crate::sexp::attrib_core::R_DimNamesSymbol(), dn);
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _cl = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"anova".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"data.frame".as_ptr()));
        crate::sexp::attrib_core::setAttrib(tab, crate::sexp::attrib_core::R_ClassSymbol(), class);
        tab
    }
}

/// GNU two-sample `power.t.test(n, delta)`.
pub unsafe fn do_power_t_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut n = NA_REAL;
        let mut delta = NA_REAL;
        let mut sd = 1.0;
        let mut sig_level = 0.05;
        let mut cell = args;
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let val = if !v.is_null() && v != R_NilValue() {
                elt_real_safe(v, 0)
            } else {
                NA_REAL
            };
            if name == "n" || (name.is_empty() && pos == 0) {
                n = val;
            } else if name == "delta" || (name.is_empty() && pos == 1) {
                delta = val;
            } else if name == "sd" || (name.is_empty() && pos == 2) {
                if val.is_finite() {
                    sd = val;
                }
            } else if name == "sig.level" || (name.is_empty() && pos == 3) {
                if val.is_finite() {
                    sig_level = val;
                }
            }
            if name.is_empty() {
                pos += 1;
            }
            cell = CDR(cell);
        }
        let nu = (n - 1.0).max(1e-7) * 2.0;
        let qu = crate::dist::t_dist::qt_inner(sig_level / 2.0, nu, false, false);
        let ncp = (n / 2.0).sqrt() * delta.abs() / sd;
        let power = crate::dist::nt_dist::pnt_inner(qu, nu, ncp, false, false);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 8);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(n));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(delta));
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(sd));
        SET_VECTOR_ELT(result, 3, Rf_ScalarReal(sig_level));
        SET_VECTOR_ELT(result, 4, Rf_ScalarReal(power));
        SET_VECTOR_ELT(result, 5, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(
            result,
            6,
            Rf_mkString(c"n is number in *each* group".as_ptr()),
        );
        SET_VECTOR_ELT(
            result,
            7,
            Rf_mkString(c"Two-sample t test power calculation".as_ptr()),
        );
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "n".to_string(),
                "delta".to_string(),
                "sd".to_string(),
                "sig.level".to_string(),
                "power".to_string(),
                "alternative".to_string(),
                "note".to_string(),
                "method".to_string(),
            ],
        );
        let class = Rf_mkString(c"power.htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU two-sample `power.prop.test(n, p1, p2)`.
pub unsafe fn do_power_prop_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut n = NA_REAL;
        let mut p1 = NA_REAL;
        let mut p2 = NA_REAL;
        let mut sig_level = 0.05;
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let val = if !v.is_null() && v != R_NilValue() {
                elt_real_safe(v, 0)
            } else {
                NA_REAL
            };
            match name.as_str() {
                "n" => n = val,
                "p1" => p1 = val,
                "p2" => p2 = val,
                "sig.level" => {
                    if val.is_finite() {
                        sig_level = val;
                    }
                }
                _ => {}
            }
            cell = CDR(cell);
        }
        let qu = crate::dist::normal::qnorm5_inner(sig_level / 2.0, 0.0, 1.0, false, false);
        let num = n.sqrt() * (p1 - p2).abs() - qu * ((p1 + p2) * (1.0 - (p1 + p2) / 2.0)).sqrt();
        let den = (p1 * (1.0 - p1) + p2 * (1.0 - p2)).sqrt();
        let power = crate::dist::normal::pnorm5_inner(num / den, 0.0, 1.0, true, false);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 8);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(n));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(p1));
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(p2));
        SET_VECTOR_ELT(result, 3, Rf_ScalarReal(sig_level));
        SET_VECTOR_ELT(result, 4, Rf_ScalarReal(power));
        SET_VECTOR_ELT(result, 5, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(
            result,
            6,
            Rf_mkString(c"n is number in *each* group".as_ptr()),
        );
        SET_VECTOR_ELT(
            result,
            7,
            Rf_mkString(c"Two-sample comparison of proportions power calculation".as_ptr()),
        );
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "n".to_string(),
                "p1".to_string(),
                "p2".to_string(),
                "sig.level".to_string(),
                "power".to_string(),
                "alternative".to_string(),
                "note".to_string(),
                "method".to_string(),
            ],
        );
        let class = Rf_mkString(c"power.htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU balanced one-way `power.anova.test`.
pub unsafe fn do_power_anova_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut groups = NA_REAL;
        let mut n = NA_REAL;
        let mut between_var = NA_REAL;
        let mut within_var = NA_REAL;
        let mut sig_level = 0.05;
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let val = if !v.is_null() && v != R_NilValue() {
                elt_real_safe(v, 0)
            } else {
                NA_REAL
            };
            match name.as_str() {
                "groups" => groups = val,
                "n" => n = val,
                "between.var" => between_var = val,
                "within.var" => within_var = val,
                "sig.level" => {
                    if val.is_finite() {
                        sig_level = val;
                    }
                }
                _ => {}
            }
            cell = CDR(cell);
        }
        let df1 = groups - 1.0;
        let df2 = (n - 1.0) * groups;
        let lambda = df1 * n * (between_var / within_var);
        let crit = crate::dist::f_dist::qf_inner(sig_level, df1, df2, false, false);
        let power = crate::dist::nf_dist::pnf_inner(crit, df1, df2, lambda, false, false);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 8);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(groups));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(n));
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(between_var));
        SET_VECTOR_ELT(result, 3, Rf_ScalarReal(within_var));
        SET_VECTOR_ELT(result, 4, Rf_ScalarReal(sig_level));
        SET_VECTOR_ELT(result, 5, Rf_ScalarReal(power));
        SET_VECTOR_ELT(
            result,
            6,
            Rf_mkString(c"n is number in each group".as_ptr()),
        );
        SET_VECTOR_ELT(
            result,
            7,
            Rf_mkString(c"Balanced one-way analysis of variance power calculation".as_ptr()),
        );
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "groups".to_string(),
                "n".to_string(),
                "between.var".to_string(),
                "within.var".to_string(),
                "sig.level".to_string(),
                "power".to_string(),
                "note".to_string(),
                "method".to_string(),
            ],
        );
        let class = Rf_mkString(c"power.htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn dist_compact(d: &[f64], i: usize, j: usize, n: usize) -> f64 {
    if i == j {
        return 0.0;
    }
    let (a, b) = if i < j { (i, j) } else { (j, i) };
    let mut idx = 0;
    for k in 0..a {
        idx += n - 1 - k;
    }
    d[idx + (b - a - 1)]
}

fn complete_link(d: &[f64], a: &[usize], b: &[usize], n: usize) -> f64 {
    let mut m = f64::NEG_INFINITY;
    for &i in a {
        for &j in b {
            m = m.max(dist_compact(d, i, j, n));
        }
    }
    m
}

/// GNU `hclust(d, method="complete")`.
pub unsafe fn do_hclust(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let d = CAR(args);
        if d.is_null() || d == R_NilValue() {
            return R_NilValue();
        }
        let size_attr = crate::sexp::attrib_core::getAttrib(
            d,
            crate::sexp::symbol::Rf_install(c"Size".as_ptr()),
        );
        let n = if !size_attr.is_null()
            && size_attr != R_NilValue()
            && TYPEOF(size_attr) == SEXPTYPE::INTSXP
            && XLENGTH(size_attr) > 0
        {
            *INTEGER(size_attr) as usize
        } else {
            let len = XLENGTH(d) as usize;
            // solve n(n-1)/2 = len
            (((1.0 + (1.0 + 8.0 * len as f64).sqrt()) / 2.0).round()) as usize
        };
        let mut dists = Vec::with_capacity(XLENGTH(d) as usize);
        for i in 0..XLENGTH(d) {
            dists.push(elt_real_safe(d, i));
        }
        #[derive(Clone)]
        struct Cl {
            members: Vec<usize>,
            id: i32,
        }
        let mut clusters: Vec<Option<Cl>> = (0..n)
            .map(|i| {
                Some(Cl {
                    members: vec![i],
                    id: -((i as i32) + 1),
                })
            })
            .collect();
        let nmerge = n - 1;
        let merge =
            crate::mainutils::array::allocMatrix(SEXPTYPE::INTSXP.as_c_int(), nmerge as i32, 2);
        let _m = protect(merge);
        let height = Rf_allocVector3(SEXPTYPE::REALSXP, nmerge as i64);
        let _h = protect(height);
        for step in 0..nmerge {
            let mut best = f64::INFINITY;
            let mut bi = 0usize;
            let mut bj = 1usize;
            for i in 0..n {
                if clusters[i].is_none() {
                    continue;
                }
                for j in (i + 1)..n {
                    if clusters[j].is_none() {
                        continue;
                    }
                    let dij = complete_link(
                        &dists,
                        &clusters[i].as_ref().unwrap().members,
                        &clusters[j].as_ref().unwrap().members,
                        n,
                    );
                    if dij < best {
                        best = dij;
                        bi = i;
                        bj = j;
                    }
                }
            }
            let a = clusters[bi].take().unwrap();
            let b = clusters[bj].take().unwrap();
            let (left, right) = match (a.id < 0, b.id < 0) {
                (true, false) => (a.id, b.id),
                (false, true) => (b.id, a.id),
                (true, true) => {
                    if a.id > b.id {
                        (a.id, b.id)
                    } else {
                        (b.id, a.id)
                    }
                }
                (false, false) => {
                    if a.id < b.id {
                        (a.id, b.id)
                    } else {
                        (b.id, a.id)
                    }
                }
            };
            *INTEGER(merge).add(step) = left;
            *INTEGER(merge).add(step + nmerge) = right;
            *REAL(height).add(step) = best;
            let mut members = a.members;
            members.extend(b.members);
            clusters[bi] = Some(Cl {
                members,
                id: (step as i32) + 1,
            });
        }
        let mut order_v = Vec::new();
        fn walk(id: i32, merge_l: &[i32], merge_r: &[i32], order: &mut Vec<i32>) {
            if id < 0 {
                order.push(-id);
            } else {
                let s = (id as usize) - 1;
                walk(merge_l[s], merge_l, merge_r, order);
                walk(merge_r[s], merge_l, merge_r, order);
            }
        }
        let mut ml = vec![0i32; nmerge];
        let mut mr = vec![0i32; nmerge];
        for s in 0..nmerge {
            ml[s] = *INTEGER(merge).add(s);
            mr[s] = *INTEGER(merge).add(s + nmerge);
        }
        walk(nmerge as i32, &ml, &mr, &mut order_v);
        let order = Rf_allocVector3(SEXPTYPE::INTSXP, n as i64);
        let _o = protect(order);
        for (i, v) in order_v.iter().enumerate() {
            *INTEGER(order).add(i) = *v;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, merge);
        SET_VECTOR_ELT(result, 1, height);
        SET_VECTOR_ELT(result, 2, order);
        SET_VECTOR_ELT(result, 3, Rf_mkString(c"complete".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "merge".to_string(),
                "height".to_string(),
                "order".to_string(),
                "method".to_string(),
            ],
        );
        let class = Rf_mkString(c"hclust".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `ecdf(x)` — empirical CDF as a step function.
pub unsafe fn do_ecdf(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let xt = TYPEOF(x);
        if xt != SEXPTYPE::INTSXP && xt != SEXPTYPE::REALSXP && xt != SEXPTYPE::LGLSXP {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'x' must be numeric",
            );
        }
        let n0 = XLENGTH(x);
        let mut vals: Vec<f64> = Vec::new();
        for i in 0..n0 {
            let v = if xt == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                let iv = *INTEGER(x).add(i as usize);
                if iv == NA_INTEGER {
                    continue;
                }
                iv as f64
            };
            if v.is_finite() {
                vals.push(v);
            }
        }
        if vals.is_empty() {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'x' must have 1 or more non-missing values",
            );
        }
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = vals.len() as f64;
        let mut uniq: Vec<f64> = Vec::new();
        let mut ys: Vec<f64> = Vec::new();
        let mut i = 0;
        while i < vals.len() {
            let v = vals[i];
            let mut j = i + 1;
            while j < vals.len() && vals[j] == v {
                j += 1;
            }
            uniq.push(v);
            ys.push(j as f64 / n);
            i = j;
        }
        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), rho, R_NilValue());
        let _env = protect(env);
        let vx = Rf_allocVector3(SEXPTYPE::REALSXP, uniq.len() as i64);
        let _vx = protect(vx);
        let vy = Rf_allocVector3(SEXPTYPE::REALSXP, ys.len() as i64);
        let _vy = protect(vy);
        for (i, v) in uniq.iter().enumerate() {
            *REAL(vx).add(i) = *v;
            *REAL(vy).add(i) = ys[i];
        }
        crate::sexp::envir::defineVar(Rf_install(c"vals".as_ptr()), vx, env);
        crate::sexp::envir::defineVar(Rf_install(c"ys".as_ptr()), vy, env);
        crate::sexp::envir::defineVar(
            Rf_install(c"nobs".as_ptr()),
            Rf_ScalarInteger(n as c_int),
            env,
        );
        let vsym = Rf_install(c"v".as_ptr());
        let formals = Rf_cons(crate::sexp::globals::R_MissingArg(), R_NilValue());
        SETTAG(formals, vsym);
        let body = crate::sexp::constructors::Rf_lang2(Rf_install(c".ecdf_apply".as_ptr()), vsym);
        let fun = crate::mainutils::dstruct::mkCLOSXP(formals, body, env);
        let _fun = protect(fun);
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"ecdf".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"stepfun".as_ptr()));
        SET_STRING_ELT(class, 2, Rf_mkChar(c"function".as_ptr()));
        crate::sexp::attrib_core::setAttrib(fun, crate::sexp::attrib_core::R_ClassSymbol(), class);
        fun
    }
}

/// Evaluate an ecdf closure: last y with vals <= v, else 0 / 1 at ends.
pub unsafe fn do_ecdf_apply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let v = CAR(args);
        let parent = crate::sexp::accessors::ENCLOS(rho);
        let vals = crate::sexp::envir::R_findVar(Rf_install(c"vals".as_ptr()), parent);
        let ys = crate::sexp::envir::R_findVar(Rf_install(c"ys".as_ptr()), parent);
        if vals.is_null()
            || ys.is_null()
            || TYPEOF(vals) != SEXPTYPE::REALSXP
            || TYPEOF(ys) != SEXPTYPE::REALSXP
        {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let nv = XLENGTH(vals) as usize;
        let nq = XLENGTH(v);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, nq);
        for i in 0..nq {
            let q = if TYPEOF(v) == SEXPTYPE::REALSXP {
                *REAL(v).add(i as usize)
            } else if TYPEOF(v) == SEXPTYPE::INTSXP {
                *INTEGER(v).add(i as usize) as f64
            } else {
                NA_REAL
            };
            let mut y = 0.0;
            for j in 0..nv {
                if *REAL(vals).add(j) <= q {
                    y = *REAL(ys).add(j);
                } else {
                    break;
                }
            }
            *REAL(result).add(i as usize) = y;
        }
        result
    }
}

/// GNU `density.default` Gaussian / nrd0 / n grid.
pub unsafe fn do_density(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let xt = TYPEOF(x);
        if xt != SEXPTYPE::INTSXP && xt != SEXPTYPE::REALSXP && xt != SEXPTYPE::LGLSXP {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "argument 'x' must be numeric",
            );
        }
        let n0 = XLENGTH(x);
        let mut xs: Vec<f64> = Vec::new();
        for i in 0..n0 {
            let v = if xt == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                let iv = *INTEGER(x).add(i as usize);
                if iv == NA_INTEGER {
                    continue;
                }
                iv as f64
            };
            if v.is_finite() {
                xs.push(v);
            }
        }
        if xs.len() < 2 {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "need at least 2 points to select a bandwidth automatically",
            );
        }
        let mut n_user: i64 = 512;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "n" {
                let v = CAR(cell);
                n_user = if TYPEOF(v) == SEXPTYPE::INTSXP {
                    *INTEGER(v) as i64
                } else if TYPEOF(v) == SEXPTYPE::REALSXP {
                    *REAL(v) as i64
                } else {
                    512
                };
            }
            cell = CDR(cell);
        }
        if n_user < 1 {
            n_user = 512;
        }
        let bw = bw_nrd0(&xs);
        let xmin = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let xmax = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let from = xmin - 3.0 * bw;
        let to = xmax + 3.0 * bw;
        let n = n_user as usize;
        let xgrid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _xg = protect(xgrid);
        let ygrid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _yg = protect(ygrid);
        let nx = xs.len() as f64;
        for i in 0..n {
            let t = if n == 1 {
                from
            } else {
                from + (to - from) * (i as f64) / ((n - 1) as f64)
            };
            *REAL(xgrid).add(i) = t;
            let mut acc = 0.0;
            for &xi in &xs {
                acc += crate::dist::normal::dnorm(t - xi, 0.0, bw, 0);
            }
            *REAL(ygrid).add(i) = acc / nx;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, xgrid);
        SET_VECTOR_ELT(result, 1, ygrid);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(bw));
        SET_VECTOR_ELT(result, 3, Rf_ScalarInteger(xs.len() as c_int));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "x".to_string(),
                "y".to_string(),
                "bw".to_string(),
                "n".to_string(),
            ],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"density".as_ptr()),
        );
        result
    }
}

fn bw_nrd0(x: &[f64]) -> f64 {
    let n = x.len() as f64;
    let mean = x.iter().sum::<f64>() / n;
    let var = x.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n - 1.0);
    let sd = var.sqrt();
    let mut xs = x.to_vec();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let iqr = if xs.len() < 2 {
        0.0
    } else {
        // type-7 IQR matching do_iqr for 1:5 → 2
        let q = |p: f64| -> f64 {
            let h = (xs.len() as f64 - 1.0) * p;
            let lo = h.floor() as usize;
            let hi = (lo + 1).min(xs.len() - 1);
            let f = h - lo as f64;
            xs[lo] * (1.0 - f) + xs[hi] * f
        };
        q(0.75) - q(0.25)
    };
    let mut lo = sd.min(iqr / 1.34);
    if lo == 0.0 {
        lo = sd;
    }
    if lo == 0.0 {
        lo = x[0].abs();
    }
    if lo == 0.0 {
        lo = 1.0;
    }
    0.9 * lo * n.powf(-0.2)
}

/// GNU `cancor` first correlation after column centering.
pub unsafe fn do_cancor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let dimx = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let dimy = crate::sexp::attrib_core::getAttrib(y, crate::sexp::attrib_core::R_DimSymbol());
        let (nrx, ncx) = if !dimx.is_null()
            && dimx != R_NilValue()
            && TYPEOF(dimx) == SEXPTYPE::INTSXP
            && XLENGTH(dimx) >= 2
        {
            (*INTEGER(dimx) as i64, *INTEGER(dimx).add(1) as i64)
        } else {
            (XLENGTH(x), 1)
        };
        let (nry, ncy) = if !dimy.is_null()
            && dimy != R_NilValue()
            && TYPEOF(dimy) == SEXPTYPE::INTSXP
            && XLENGTH(dimy) >= 2
        {
            (*INTEGER(dimy) as i64, *INTEGER(dimy).add(1) as i64)
        } else {
            (XLENGTH(y), 1)
        };
        if nrx != nry {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "unequal number of rows in 'cancor'",
            );
        }
        if nrx == 0 || ncx == 0 || ncy == 0 {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "dimension 0 in 'x' or 'y'",
            );
        }
        let mut mx = vec![0.0; ncx as usize];
        let mut my = vec![0.0; ncy as usize];
        for j in 0..ncx {
            let mut s = 0.0;
            for i in 0..nrx {
                s += matrix_real(x, i + j * nrx);
            }
            mx[j as usize] = s / nrx as f64;
        }
        for j in 0..ncy {
            let mut s = 0.0;
            for i in 0..nry {
                s += matrix_real(y, i + j * nry);
            }
            my[j as usize] = s / nry as f64;
        }
        let mut sx = 0.0;
        let mut sy = 0.0;
        let mut sxy = 0.0;
        for i in 0..nrx {
            let a = matrix_real(x, i) - mx[0];
            let b = matrix_real(y, i) - my[0];
            sx += a * a;
            sy += b * b;
            sxy += a * b;
        }
        let cor = if sx > 0.0 && sy > 0.0 {
            sxy / (sx.sqrt() * sy.sqrt())
        } else {
            0.0
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(cor));
        let xcoef = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        *REAL(xcoef) = 1.0;
        let ycoef = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        *REAL(ycoef) = 1.0;
        let dx = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(dx) = 1;
        *INTEGER(dx).add(1) = 1;
        crate::sexp::attrib_core::setAttrib(xcoef, crate::sexp::attrib_core::R_DimSymbol(), dx);
        let dy = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(dy) = 1;
        *INTEGER(dy).add(1) = 1;
        crate::sexp::attrib_core::setAttrib(ycoef, crate::sexp::attrib_core::R_DimSymbol(), dy);
        SET_VECTOR_ELT(result, 1, xcoef);
        SET_VECTOR_ELT(result, 2, ycoef);
        let xc = Rf_allocVector3(SEXPTYPE::REALSXP, ncx);
        for (i, m) in mx.iter().enumerate() {
            *REAL(xc).add(i) = *m;
        }
        let yc = Rf_allocVector3(SEXPTYPE::REALSXP, ncy);
        for (i, m) in my.iter().enumerate() {
            *REAL(yc).add(i) = *m;
        }
        SET_VECTOR_ELT(result, 3, xc);
        SET_VECTOR_ELT(result, 4, yc);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "cor".to_string(),
                "xcoef".to_string(),
                "ycoef".to_string(),
                "xcenter".to_string(),
                "ycenter".to_string(),
            ],
        );
        result
    }
}

unsafe fn matrix_real(x: SEXP, i: i64) -> f64 {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else {
            *INTEGER(x).add(i as usize) as f64
        }
    }
}

/// GNU `dist(x)` Euclidean for a numeric vector.
pub unsafe fn do_dist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x);
        let len = n * (n - 1) / 2;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, len);
        let _r = protect(result);
        let mut k = 0usize;
        for i in 0..n {
            for j in (i + 1)..n {
                let a = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(i as usize)
                } else {
                    *INTEGER(x).add(i as usize) as f64
                };
                let b = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(j as usize)
                } else {
                    *INTEGER(x).add(j as usize) as f64
                };
                *REAL(result).add(k) = (a - b).abs();
                k += 1;
            }
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(c"Size".as_ptr()),
            Rf_ScalarInteger(n as c_int),
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(c"method".as_ptr()),
            Rf_mkString(c"euclidean".as_ptr()),
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"dist".as_ptr()),
        );
        result
    }
}

/// GNU `prcomp` via eigen of the sample covariance.
pub unsafe fn do_prcomp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (nr, nc) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            (XLENGTH(x) as usize, 1)
        };
        if nr < 2 || nc < 1 {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "cannot rescale a constant/zero column to unit variance",
            );
        }
        let mut data = vec![0.0f64; nr * nc];
        for j in 0..nc {
            let mut mean = 0.0;
            for i in 0..nr {
                let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(i + j * nr)
                } else {
                    *INTEGER(x).add(i + j * nr) as f64
                };
                data[i + j * nr] = v;
                mean += v;
            }
            mean /= nr as f64;
            for i in 0..nr {
                data[i + j * nr] -= mean;
            }
        }
        let cov = Rf_allocVector3(SEXPTYPE::REALSXP, (nc * nc) as i64);
        let _c = protect(cov);
        let cdim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(cdim) = nc as c_int;
        *INTEGER(cdim).add(1) = nc as c_int;
        crate::sexp::attrib_core::setAttrib(cov, crate::sexp::attrib_core::R_DimSymbol(), cdim);
        let denom = (nr - 1) as f64;
        for a in 0..nc {
            for b in 0..nc {
                let mut s = 0.0;
                for i in 0..nr {
                    s += data[i + a * nr] * data[i + b * nr];
                }
                *REAL(cov).add(a + b * nc) = s / denom;
            }
        }
        let ev_args = Rf_cons(cov, R_NilValue());
        let _ea = protect(ev_args);
        let ev = crate::mainutils::eigen::do_eigen(_call, _op, ev_args, _rho);
        let _ev = protect(ev);
        let values = VECTOR_ELT(ev, 0);
        let vectors = VECTOR_ELT(ev, 1);
        let sdev = Rf_allocVector3(SEXPTYPE::REALSXP, nc as i64);
        for j in 0..nc {
            let lam = if TYPEOF(values) == SEXPTYPE::REALSXP {
                *REAL(values).add(j)
            } else {
                0.0
            };
            *REAL(sdev).add(j) = if lam > 1e-10 { lam.sqrt() } else { 0.0 };
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, sdev);
        SET_VECTOR_ELT(result, 1, vectors);
        crate::mainutils::essentials::set_string_names(
            result,
            &["sdev".to_string(), "rotation".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"prcomp".as_ptr()),
        );
        result
    }
}

/// GNU `princomp` via eigen of the /n covariance.
pub unsafe fn do_princomp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (nr, nc) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            (XLENGTH(x) as usize, 1)
        };
        if nr <= nc {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'princomp' can only be used with more units than variables",
            );
        }
        let mut data = vec![0.0f64; nr * nc];
        for j in 0..nc {
            let mut mean = 0.0;
            for i in 0..nr {
                let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(i + j * nr)
                } else {
                    *INTEGER(x).add(i + j * nr) as f64
                };
                data[i + j * nr] = v;
                mean += v;
            }
            mean /= nr as f64;
            for i in 0..nr {
                data[i + j * nr] -= mean;
            }
        }
        let cov = Rf_allocVector3(SEXPTYPE::REALSXP, (nc * nc) as i64);
        let _c = protect(cov);
        let cdim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(cdim) = nc as c_int;
        *INTEGER(cdim).add(1) = nc as c_int;
        crate::sexp::attrib_core::setAttrib(cov, crate::sexp::attrib_core::R_DimSymbol(), cdim);
        let denom = nr as f64;
        for a in 0..nc {
            for b in 0..nc {
                let mut s = 0.0;
                for i in 0..nr {
                    s += data[i + a * nr] * data[i + b * nr];
                }
                *REAL(cov).add(a + b * nc) = s / denom;
            }
        }
        let ev_args = Rf_cons(cov, R_NilValue());
        let _ea = protect(ev_args);
        let ev = crate::mainutils::eigen::do_eigen(_call, _op, ev_args, _rho);
        let _ev = protect(ev);
        let values = VECTOR_ELT(ev, 0);
        let vectors = VECTOR_ELT(ev, 1);
        let sdev = Rf_allocVector3(SEXPTYPE::REALSXP, nc as i64);
        for j in 0..nc {
            let lam = if TYPEOF(values) == SEXPTYPE::REALSXP {
                *REAL(values).add(j)
            } else {
                0.0
            };
            *REAL(sdev).add(j) = if lam > 1e-16 { lam.sqrt() } else { 0.0 };
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, sdev);
        SET_VECTOR_ELT(result, 1, vectors);
        crate::mainutils::essentials::set_string_names(
            result,
            &["sdev".to_string(), "loadings".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"princomp".as_ptr()),
        );
        result
    }
}

/// GNU `wilcox.test(x, y)` two-sample rank-sum.
pub unsafe fn do_wilcox_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let nx = XLENGTH(x);
        let ny = XLENGTH(y);
        let mut vals: Vec<(f64, u8)> = Vec::new();
        for i in 0..nx {
            let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                *INTEGER(x).add(i as usize) as f64
            };
            vals.push((v, 0));
        }
        for i in 0..ny {
            let v = if TYPEOF(y) == SEXPTYPE::REALSXP {
                *REAL(y).add(i as usize)
            } else {
                *INTEGER(y).add(i as usize) as f64
            };
            vals.push((v, 1));
        }
        vals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let n = vals.len();
        let mut ranks = vec![0.0f64; n];
        let mut i = 0;
        while i < n {
            let mut j = i + 1;
            while j < n && vals[j].0 == vals[i].0 {
                j += 1;
            }
            let r = (i + 1 + j) as f64 / 2.0;
            for k in i..j {
                ranks[k] = r;
            }
            i = j;
        }
        let mut wx = 0.0;
        for (k, (_, which)) in vals.iter().enumerate() {
            if *which == 0 {
                wx += ranks[k];
            }
        }
        let stat = wx - (nx as f64) * (nx as f64 + 1.0) / 2.0;
        let p_lo =
            crate::nmath::dist::wilcox::pwilcox_inner(stat, nx as f64, ny as f64, true, false);
        let p_hi = crate::nmath::dist::wilcox::pwilcox_inner(
            stat - 1e-9,
            nx as f64,
            ny as f64,
            false,
            false,
        );
        let p = (2.0 * p_lo.min(p_hi)).min(1.0);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(stat));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(p));
        crate::mainutils::essentials::set_string_names(
            result,
            &["statistic".to_string(), "p.value".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"htest".as_ptr()),
        );
        result
    }
}

/// GNU `ks.test(x, "punif", min, max)` one-sample.
pub unsafe fn do_ks_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mut min = 0.0;
        let mut max = 1.0;
        let mut cell = CDR(CDR(args));
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            let val = if TYPEOF(v) == SEXPTYPE::REALSXP {
                *REAL(v)
            } else if TYPEOF(v) == SEXPTYPE::INTSXP {
                *INTEGER(v) as f64
            } else {
                0.0
            };
            if pos == 0 {
                min = val;
            } else if pos == 1 {
                max = val;
            }
            pos += 1;
            cell = CDR(cell);
        }
        let n = XLENGTH(x);
        let mut xs: Vec<f64> = Vec::new();
        for i in 0..n {
            let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                *INTEGER(x).add(i as usize) as f64
            };
            if v.is_finite() {
                xs.push(v);
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = xs.len() as f64;
        let span = if max > min { max - min } else { 1.0 };
        let mut d = 0.0f64;
        for (i, xi) in xs.iter().enumerate() {
            let f = ((*xi - min) / span).clamp(0.0, 1.0);
            let fn_plus = (i as f64 + 1.0) / n;
            let fn_minus = i as f64 / n;
            d = d.max((fn_plus - f).abs()).max((fn_minus - f).abs());
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(d));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(1.0));
        crate::mainutils::essentials::set_string_names(
            result,
            &["statistic".to_string(), "p.value".to_string()],
        );
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"ks.test".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"htest".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `Box.test` Box-Pierce lag 1.
pub unsafe fn do_box_test(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x);
        let lag_s = Rf_ScalarInteger(1);
        let _ls = protect(lag_s);
        let acf_args = Rf_cons(x, Rf_cons(lag_s, R_NilValue()));
        SETTAG(CDR(acf_args), Rf_install(c"lag.max".as_ptr()));
        let _aa = protect(acf_args);
        let a = crate::library::stats::filter::do_acf(_call, _op, acf_args, rho);
        let _a = protect(a);
        let acfv = VECTOR_ELT(a, 0);
        let rho1 = if XLENGTH(acfv) >= 2 {
            *REAL(acfv).add(1)
        } else {
            0.0
        };
        let stat = n as f64 * rho1 * rho1;
        let p = 1.0 - crate::nmath::dist::chisq::pchisq_inner(stat, 1.0, true, false);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(stat));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(p));
        crate::mainutils::essentials::set_string_names(
            result,
            &["statistic".to_string(), "p.value".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"htest".as_ptr()),
        );
        result
    }
}

// ---------------------------------------------------------------------------
// Critical remaining R functions
// ---------------------------------------------------------------------------

/// R sample.int(n, size = n, replace = FALSE) — uniform sampling from 1:n.
pub unsafe fn do_sample_int(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = real_or_default(CAR(args), 1.0) as i64;
        let size = CAR(CDR(args));
        let replace = CAR(CDR(CDR(args)));
        let prob = CAR(CDR(CDR(CDR(args))));
        crate::mainutils::rng_dispatch::sample_int_values(n, size, replace, prob)
    }
}

/// R setNames(object, nm)
pub unsafe fn do_setNames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let nm = CAR(CDR(args));
        if obj.is_null() || nm.is_null() {
            return obj;
        }
        crate::sexp::attrib_core::setAttrib(obj, Rf_install(c"names".as_ptr()), nm);
        obj
    }
}

/// R toString(x)
pub unsafe fn do_toString(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_mkString(c"".as_ptr());
        }
        let n = XLENGTH(x);
        let mut parts: Vec<String> = Vec::new();
        for i in 0..n.min(999) {
            parts.push(elt_to_string(x, i));
        }
        if n > 999 {
            parts.push("...".to_string());
        }
        Rf_mkString(CString::new(parts.join(", ")).unwrap_or_default().as_ptr())
    }
}

/// R normalizePath(path)
pub unsafe fn do_normalizePath(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut path_arg = R_NilValue();
        let mut must_work_arg = R_NilValue();
        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match tag_name(current).as_deref() {
                Some("path") => path_arg = value,
                Some("mustWork") => must_work_arg = value,
                Some("winslash") => {}
                Some(_) => {}
                None => {
                    match positional {
                        0 => path_arg = value,
                        1 => {}
                        2 => must_work_arg = value,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        if path_arg.is_null() || path_arg == R_NilValue() {
            return R_NilValue();
        }

        let must_work = if must_work_arg.is_null()
            || must_work_arg == R_NilValue()
            || XLENGTH(must_work_arg) == 0
        {
            NA_INTEGER
        } else {
            *LOGICAL(must_work_arg)
        };

        let n = XLENGTH(path_arg);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _result_guard = protect(result);
        for i in 0..n {
            let elt = STRING_ELT(path_arg, i);
            if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }

            let path = CStr::from_ptr(CHAR(elt)).to_str().unwrap_or("").to_string();
            match std::fs::canonicalize(&path) {
                Ok(p) => SET_STRING_ELT(
                    result,
                    i,
                    crate::sexp::constructors::Rf_mkChar(
                        CString::new(p.to_string_lossy().as_ref())
                            .unwrap_or_default()
                            .as_ptr(),
                    ),
                ),
                Err(err) => {
                    if must_work == TRUE {
                        base_error(format!("path[{}]=\"{}\": {}", i + 1, path, err));
                    }
                    SET_STRING_ELT(
                        result,
                        i,
                        crate::sexp::constructors::Rf_mkChar(
                            CString::new(path).unwrap_or_default().as_ptr(),
                        ),
                    );
                }
            }
        }
        result
    }
}

/// R tempfile(pattern = "file", tmpdir = tempdir(), fileext = "")
pub unsafe fn do_tempfile(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut pattern = "file".to_string();
        let mut tmpdir: Option<PathBuf> = None;
        let mut fileext = String::new();
        // Upstream tempfile(pattern, tmpdir, fileext): arguments match by
        // TAG first (tempfile(fileext = ".R") leaves pattern at its
        // "file" default), then by position.
        let mut positional: Vec<SEXP> = Vec::new();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let value = CAR(cell);
            let tagged = !tag.is_null() && tag != R_NilValue();
            if tagged {
                let name = std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(
                    crate::sexp::accessors::PRINTNAME(tag),
                ))
                .to_string_lossy()
                .into_owned();
                let nonempty = !value.is_null() && value != R_NilValue() && XLENGTH(value) > 0;
                match name.as_str() {
                    "pattern" if nonempty => pattern = elt_to_string(value, 0),
                    "tmpdir" if nonempty => tmpdir = Some(PathBuf::from(elt_to_string(value, 0))),
                    "fileext" if nonempty => fileext = elt_to_string(value, 0),
                    _ => {}
                }
            } else {
                positional.push(value);
            }
            cell = CDR(cell);
        }
        for (i, value) in positional.iter().enumerate() {
            if value.is_null() || *value == R_NilValue() || XLENGTH(*value) == 0 {
                continue;
            }
            match i {
                0 => pattern = elt_to_string(*value, 0),
                1 => tmpdir = Some(PathBuf::from(elt_to_string(*value, 0))),
                2 => fileext = elt_to_string(*value, 0),
                _ => {}
            }
        }
        let default_tmp = crate::sexp::instance::with_required_current_instance(|inst| {
            (*inst).path_policy.temp_dir().to_path_buf()
        });
        let tmp = tmpdir.unwrap_or(default_tmp);
        let path =
            crate::mainutils::sysutils::R_tmpnam2(&pattern, &tmp.to_string_lossy(), &fileext)
                .unwrap_or_else(|| base_error("cannot find an unused temporary filename"));
        Rf_mkString(CString::new(path.as_str()).unwrap_or_default().as_ptr())
    }
}

/// R tempdir()
pub unsafe fn do_tempdir(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let temp_dir = session_temp_dir();
        let _ = std::fs::create_dir_all(&temp_dir);
        Rf_mkString(
            CString::new(temp_dir.to_string_lossy().as_ref())
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

fn session_temp_dir() -> PathBuf {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).path_policy.temp_dir().to_path_buf()
    })
}

/// R proc.time()
pub unsafe fn do_proc_time(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 5);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for i in 0..5 {
            *REAL(result).add(i) = 0.0;
        }
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 5);
        if !names.is_null() {
            let _np = protect(names);
            for (i, name) in [
                "user.self",
                "sys.self",
                "elapsed",
                "user.child",
                "sys.child",
            ]
            .iter()
            .enumerate()
            {
                let cstr = CString::new(*name).unwrap_or_default();
                SET_STRING_ELT(names, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
        }
        let class = Rf_mkString(c"proc_time".as_ptr());
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class);
        result
    }
}

/// R regexpr(pattern, text) — port of grep.c:do_regexpr.
///
/// With perl = TRUE and capture groups in the pattern, attaches the
/// capture.start / capture.length matrices (one row per text element, one
/// column per group, column-major, dimnames list(NULL, capture.names)) and
/// the capture.names vector, mirroring grep.c's pcre2_pattern_info +
/// extract_match_and_groups: non-participating groups yield start 0 /
/// length 0 (PCRE2_UNSET arithmetic), non-matching elements -1 / -1, and
/// NA text elements leave the NA initialization (PR#16484).
pub unsafe fn do_regexpr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pat = elt_to_string(CAR(args), 0);
        let text = CAR(CDR(args));
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let perl = named_logical_arg(args, "perl").unwrap_or(false);
        let fixed = named_logical_arg(args, "fixed").unwrap_or(false);
        let n = XLENGTH(text);

        // grep.c drops perl when fixed = TRUE, so only a genuine perl run
        // gets capture attribution. A pattern that fails to compile here
        // also fails the per-element match below (reported as no match).
        let (capture_count, capture_names) = if perl && !fixed {
            crate::mainutils::grep::perl_group_info(&pat, ignore_case)
                .unwrap_or_else(|| (0, Vec::new()))
        } else {
            (0, Vec::new())
        };

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let match_len = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if match_len.is_null() {
            return R_NilValue();
        }
        let _mlp = protect(match_len);

        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(c"match.length".as_ptr()),
            match_len,
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(c"index.type".as_ptr()),
            Rf_mkString(c"chars".as_ptr()),
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(c"useBytes".as_ptr()),
            Rf_ScalarLogical(TRUE),
        );

        // Capture matrices, initialized to NA so NA text elements keep NA
        // entries (grep.c PR#16484); overwritten per match below.
        let mut capture_start = R_NilValue();
        let mut capture_len = R_NilValue();
        if capture_count > 0 {
            let names_sexp = Rf_allocVector3(SEXPTYPE::STRSXP, capture_count as R_xlen_t);
            if names_sexp.is_null() {
                return R_NilValue();
            }
            let _nsg = protect(names_sexp);
            for (g, name) in capture_names.iter().enumerate() {
                let cstr = CString::new(name.as_str()).unwrap_or_default();
                SET_STRING_ELT(names_sexp, g as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
            }

            capture_start = alloc_int_matrix(n, capture_count);
            if capture_start.is_null() {
                return R_NilValue();
            }
            let _csp = protect(capture_start);
            capture_len = alloc_int_matrix(n, capture_count);
            if capture_len.is_null() {
                return R_NilValue();
            }
            let _clp = protect(capture_len);

            let dmn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            if dmn.is_null() {
                return R_NilValue();
            }
            let _dmp = protect(dmn);
            SET_VECTOR_ELT(dmn, 0, R_NilValue());
            SET_VECTOR_ELT(dmn, 1, names_sexp);
            crate::sexp::attrib_core::setAttrib(
                capture_start,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dmn,
            );
            crate::sexp::attrib_core::setAttrib(
                capture_len,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dmn,
            );

            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(c"capture.start".as_ptr()),
                capture_start,
            );
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(c"capture.length".as_ptr()),
                capture_len,
            );
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(c"capture.names".as_ptr()),
                names_sexp,
            );

            let total = (n as usize) * capture_count;
            for j in 0..total {
                *INTEGER(capture_start).add(j) = NA_INTEGER;
                *INTEGER(capture_len).add(j) = NA_INTEGER;
            }
        }

        for i in 0..n {
            // grep.c: NA text elements yield NA, capture entries stay NA.
            if TYPEOF(text) == SEXPTYPE::STRSXP
                && STRING_ELT(text, i) == crate::sexp::globals::R_NaString()
            {
                *INTEGER(result).add(i as usize) = NA_INTEGER;
                *INTEGER(match_len).add(i as usize) = NA_INTEGER;
                continue;
            }

            let txt = elt_to_string(text, i);
            if fixed {
                match fixed_find(&txt, &pat, ignore_case) {
                    Some(m) => {
                        *INTEGER(result).add(i as usize) = (m.start + 1) as c_int;
                        *INTEGER(match_len).add(i as usize) = (m.end - m.start) as c_int;
                    }
                    None => {
                        *INTEGER(result).add(i as usize) = -1;
                        *INTEGER(match_len).add(i as usize) = -1;
                    }
                }
            } else if capture_count > 0 {
                // perl run with capture groups: one full-capture match
                // fills the overall position and every group column.
                let caps = crate::mainutils::grep::perl_captures(&pat, &txt, ignore_case);
                match caps
                    .as_ref()
                    .and_then(|c| c.first())
                    .and_then(|c| c.as_ref())
                {
                    Some(whole) => {
                        *INTEGER(result).add(i as usize) = (whole.start + 1) as c_int;
                        *INTEGER(match_len).add(i as usize) = (whole.end - whole.start) as c_int;
                        for g in 0..capture_count {
                            let ind = i as usize + g * n as usize;
                            match caps
                                .as_ref()
                                .and_then(|c| c.get(g + 1))
                                .and_then(|c| c.as_ref())
                            {
                                Some(cm) => {
                                    *INTEGER(capture_start).add(ind) = (cm.start + 1) as c_int;
                                    *INTEGER(capture_len).add(ind) = (cm.end - cm.start) as c_int;
                                }
                                None => {
                                    // grep.c's ovector_extract_start_length on
                                    // a PCRE2_UNSET group computes
                                    // start = -1 + 1 = 0 and length = 0.
                                    *INTEGER(capture_start).add(ind) = 0;
                                    *INTEGER(capture_len).add(ind) = 0;
                                }
                            }
                        }
                    }
                    None => {
                        *INTEGER(result).add(i as usize) = -1;
                        *INTEGER(match_len).add(i as usize) = -1;
                        fill_capture_no_match(
                            capture_start,
                            capture_len,
                            i as usize,
                            n,
                            capture_count,
                        );
                    }
                }
            } else {
                let found = if perl {
                    crate::mainutils::grep::perl_find(&pat, &txt, ignore_case)
                } else {
                    crate::mainutils::grep::ere_find(&pat, &txt, ignore_case)
                };
                match found {
                    Some(m) => {
                        *INTEGER(result).add(i as usize) = (m.start + 1) as c_int;
                        *INTEGER(match_len).add(i as usize) = (m.end - m.start) as c_int;
                    }
                    None => {
                        *INTEGER(result).add(i as usize) = -1;
                        *INTEGER(match_len).add(i as usize) = -1;
                    }
                }
            }
        }

        result
    }
}

/// Column-major integer matrix with dim c(nrow, ncol) — the shape grep.c's
/// do_regexpr uses for capture.start / capture.length.
unsafe fn alloc_int_matrix(nrow: R_xlen_t, ncol: usize) -> SEXP {
    unsafe {
        let ans = Rf_allocVector3(SEXPTYPE::INTSXP, nrow * ncol as R_xlen_t);
        if ans.is_null() {
            return ans;
        }
        let _ans_guard = protect(ans);
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if dim.is_null() {
            return R_NilValue();
        }
        let _dim_guard = protect(dim);
        *INTEGER(dim) = nrow as c_int;
        *INTEGER(dim).add(1) = ncol as c_int;
        crate::sexp::attrib_core::setAttrib(ans, crate::sexp::attrib_core::R_DimSymbol(), dim);
        ans
    }
}

/// Mark every capture entry of a non-matching text element as -1
/// (grep.c's no-match branch in do_regexpr).
unsafe fn fill_capture_no_match(
    capture_start: SEXP,
    capture_len: SEXP,
    i: usize,
    n: R_xlen_t,
    capture_count: usize,
) {
    unsafe {
        if capture_start == R_NilValue() {
            return;
        }
        for g in 0..capture_count {
            let ind = i + g * n as usize;
            *INTEGER(capture_start).add(ind) = -1;
            *INTEGER(capture_len).add(ind) = -1;
        }
    }
}

/// R gregexpr(pattern, text) for repeated non-overlapping matches. With
/// perl = TRUE and capture groups, each element carries the
/// capture.start / capture.length matrices (one row per match, one column
/// per group) and capture.names, mirroring grep.c's perl branch of
/// do_gregexpr; non-matching elements get one row of -1s.
pub unsafe fn do_gregexpr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pat = elt_to_string(CAR(args), 0);
        let text = CAR(CDR(args));
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let perl = named_logical_arg(args, "perl").unwrap_or(false);
        let fixed = named_logical_arg(args, "fixed").unwrap_or(false);
        // grep.c drops perl when fixed = TRUE, so only a genuine perl run
        // gets capture attribution (same guard as do_regexpr).
        let (capture_count, capture_names) = if perl && !fixed {
            crate::mainutils::grep::perl_group_info(&pat, ignore_case)
                .unwrap_or_else(|| (0, Vec::new()))
        } else {
            (0, Vec::new())
        };
        let n = XLENGTH(text);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            let txt = elt_to_string(text, i);
            let mut starts = Vec::new();
            let mut lengths = Vec::new();
            // Per-match group spans (0-based start, length); None mirrors
            // PCRE2_UNSET groups (0/0 arithmetic below).
            let mut group_spans: Vec<Vec<Option<(usize, usize)>>> = Vec::new();
            if !pat.is_empty() {
                if capture_count > 0 {
                    // Perl run with capture groups: every global match
                    // contributes its whole-match span and its groups.
                    if let Some(all) =
                        crate::mainutils::grep::perl_captures_all(&pat, &txt, ignore_case)
                    {
                        for caps in all {
                            if let Some(whole) = caps.first().and_then(|c| c.as_ref()) {
                                starts.push(whole.start + 1);
                                lengths.push(whole.end - whole.start);
                                group_spans.push(
                                    (1..=capture_count)
                                        .map(|g| {
                                            caps.get(g)
                                                .and_then(|c| c.as_ref())
                                                .map(|m| (m.start, m.end - m.start))
                                        })
                                        .collect(),
                                );
                            }
                        }
                    }
                } else {
                    let mut offset = 0usize;
                    while offset <= txt.len() {
                        let hay = &txt[offset..];
                        let found = if fixed {
                            fixed_find(hay, &pat, ignore_case)
                        } else if perl {
                            crate::mainutils::grep::perl_find(&pat, hay, ignore_case)
                        } else {
                            crate::mainutils::grep::ere_find(&pat, hay, ignore_case)
                        };
                        let Some(m) = found else {
                            break;
                        };
                        let start = offset + m.start;
                        starts.push(start + 1);
                        lengths.push(m.end - m.start);
                        let next_offset = offset + m.end;
                        offset = if m.start == m.end {
                            next_offset
                                + txt[next_offset..].chars().next().map_or(1, char::len_utf8)
                        } else {
                            next_offset
                        };
                    }
                }
            }

            let (elt, match_lengths) = if starts.is_empty() {
                let elt = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                let match_lengths = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                if elt.is_null() || match_lengths.is_null() {
                    return R_NilValue();
                }
                let _elt_guard = protect(elt);
                let _ml_guard = protect(match_lengths);
                *INTEGER(elt) = -1;
                *INTEGER(match_lengths) = -1;
                (elt, match_lengths)
            } else {
                let elt = Rf_allocVector3(SEXPTYPE::INTSXP, starts.len() as R_xlen_t);
                let match_lengths = Rf_allocVector3(SEXPTYPE::INTSXP, starts.len() as R_xlen_t);
                if elt.is_null() || match_lengths.is_null() {
                    return R_NilValue();
                }
                let _elt_guard = protect(elt);
                let _ml_guard = protect(match_lengths);
                for (idx, start) in starts.iter().enumerate() {
                    *INTEGER(elt).add(idx) = *start as c_int;
                    *INTEGER(match_lengths).add(idx) = lengths[idx] as c_int;
                }
                (elt, match_lengths)
            };

            set_regexpr_attrs(elt, match_lengths);
            if capture_count > 0 {
                if starts.is_empty() {
                    // grep.c's no-match branch: one row of -1s per group.
                    set_gregexpr_capture_attrs(elt, 1, capture_count, &capture_names, |_, _| {
                        (-1, -1)
                    });
                } else {
                    let spans = &group_spans;
                    set_gregexpr_capture_attrs(
                        elt,
                        starts.len(),
                        capture_count,
                        &capture_names,
                        |k, g| spans[k][g].map_or((0, 0), |(s, l)| (s as c_int + 1, l as c_int)),
                    );
                }
            }
            SET_VECTOR_ELT(result, i, elt);
        }

        result
    }
}

fn substring_chars(text: &str, start: i32, end: i32) -> String {
    if start < 1 || end < start {
        return String::new();
    }
    text.chars()
        .skip((start - 1) as usize)
        .take((end - start + 1) as usize)
        .collect()
}

/// GNU `regmatches(x, m)` for invert = FALSE.
pub unsafe fn do_regmatches(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let m = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || m.is_null() || m == R_NilValue() {
            return R_NilValue();
        }
        let ml_sym = crate::sexp::symbol::Rf_install(c"match.length".as_ptr());
        if TYPEOF(m) == SEXPTYPE::VECSXP {
            let n = XLENGTH(m);
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
            let _r = protect(result);
            for i in 0..n {
                let starts = VECTOR_ELT(m, i);
                let lengths = crate::sexp::attrib_core::getAttrib(starts, ml_sym);
                let text = elt_to_string(x, i);
                let ns = if starts.is_null() || starts == R_NilValue() {
                    0
                } else {
                    XLENGTH(starts)
                };
                let mut parts: Vec<String> = Vec::new();
                for j in 0..ns {
                    let so = if TYPEOF(starts) == SEXPTYPE::INTSXP {
                        *INTEGER(starts).add(j as usize)
                    } else {
                        -1
                    };
                    if so <= 0 {
                        continue;
                    }
                    let ml = if !lengths.is_null()
                        && lengths != R_NilValue()
                        && TYPEOF(lengths) == SEXPTYPE::INTSXP
                        && XLENGTH(lengths) > j
                    {
                        *INTEGER(lengths).add(j as usize)
                    } else {
                        0
                    };
                    parts.push(substring_chars(&text, so, so + ml - 1));
                }
                let elt = Rf_allocVector3(SEXPTYPE::STRSXP, parts.len() as i64);
                let _e = protect(elt);
                for (j, p) in parts.iter().enumerate() {
                    let c = CString::new(p.as_str()).unwrap_or_default();
                    SET_STRING_ELT(elt, j as i64, Rf_mkChar(c.as_ptr()));
                }
                SET_VECTOR_ELT(result, i, elt);
            }
            result
        } else {
            let n = XLENGTH(m);
            let lengths = crate::sexp::attrib_core::getAttrib(m, ml_sym);
            let mut parts: Vec<String> = Vec::new();
            for i in 0..n {
                let so = if TYPEOF(m) == SEXPTYPE::INTSXP {
                    *INTEGER(m).add(i as usize)
                } else {
                    -1
                };
                if so <= 0 {
                    continue;
                }
                let ml = if !lengths.is_null()
                    && lengths != R_NilValue()
                    && TYPEOF(lengths) == SEXPTYPE::INTSXP
                    && XLENGTH(lengths) > i
                {
                    *INTEGER(lengths).add(i as usize)
                } else {
                    0
                };
                let text = elt_to_string(x, i);
                parts.push(substring_chars(&text, so, so + ml - 1));
            }
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, parts.len() as i64);
            let _r = protect(result);
            for (j, p) in parts.iter().enumerate() {
                let c = CString::new(p.as_str()).unwrap_or_default();
                SET_STRING_ELT(result, j as i64, Rf_mkChar(c.as_ptr()));
            }
            result
        }
    }
}

/// Attach capture.start / capture.length / capture.names attrs to one
/// gregexpr element: n_match x capture_count column-major matrices with
/// dimnames list(NULL, names) — the shape grep.c's do_gregexpr builds per
/// element. `span` maps (match_idx, group_idx) to (start, length).
unsafe fn set_gregexpr_capture_attrs(
    elt: SEXP,
    n_match: usize,
    capture_count: usize,
    capture_names: &[String],
    span: impl Fn(usize, usize) -> (c_int, c_int),
) {
    unsafe {
        let names_sexp = Rf_allocVector3(SEXPTYPE::STRSXP, capture_count as R_xlen_t);
        if names_sexp.is_null() {
            return;
        }
        let _nsg = protect(names_sexp);
        for (g, name) in capture_names.iter().enumerate() {
            let cstr = CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(names_sexp, g as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }

        let capture_start = alloc_int_matrix(n_match as R_xlen_t, capture_count);
        let capture_len = alloc_int_matrix(n_match as R_xlen_t, capture_count);
        if capture_start.is_null() || capture_len.is_null() {
            return;
        }
        let _csp = protect(capture_start);
        let _clp = protect(capture_len);
        for k in 0..n_match {
            for g in 0..capture_count {
                let (start, len) = span(k, g);
                let ind = k + g * n_match;
                *INTEGER(capture_start).add(ind) = start;
                *INTEGER(capture_len).add(ind) = len;
            }
        }

        let dmn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if dmn.is_null() {
            return;
        }
        let _dmp = protect(dmn);
        SET_VECTOR_ELT(dmn, 0, R_NilValue());
        SET_VECTOR_ELT(dmn, 1, names_sexp);
        crate::sexp::attrib_core::setAttrib(
            capture_start,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dmn,
        );
        crate::sexp::attrib_core::setAttrib(
            capture_len,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dmn,
        );

        crate::sexp::attrib_core::setAttrib(
            elt,
            Rf_install(c"capture.start".as_ptr()),
            capture_start,
        );
        crate::sexp::attrib_core::setAttrib(
            elt,
            Rf_install(c"capture.length".as_ptr()),
            capture_len,
        );
        crate::sexp::attrib_core::setAttrib(elt, Rf_install(c"capture.names".as_ptr()), names_sexp);
    }
}

/// R regexec(pattern, text) for the overall fixed match.
pub unsafe fn do_regexec(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pat = elt_to_string(CAR(args), 0);
        let text = CAR(CDR(args));
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let perl = named_logical_arg(args, "perl").unwrap_or(false);
        let fixed = named_logical_arg(args, "fixed").unwrap_or(false);
        let n = XLENGTH(text);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            let txt = elt_to_string(text, i);
            let captures = if fixed {
                fixed_find(&txt, &pat, ignore_case).map(|m| vec![Some(m)])
            } else if perl {
                crate::mainutils::grep::perl_captures(&pat, &txt, ignore_case)
            } else {
                crate::mainutils::grep::ere_captures(&pat, &txt, ignore_case)
            };
            let (elt, match_lengths) = if let Some(captures) = captures {
                let elt = Rf_allocVector3(SEXPTYPE::INTSXP, captures.len() as R_xlen_t);
                let match_lengths = Rf_allocVector3(SEXPTYPE::INTSXP, captures.len() as R_xlen_t);
                if elt.is_null() || match_lengths.is_null() {
                    return R_NilValue();
                }
                let _elt_guard = protect(elt);
                let _ml_guard = protect(match_lengths);
                for (idx, capture) in captures.iter().enumerate() {
                    if let Some(m) = capture {
                        *INTEGER(elt).add(idx) = (m.start + 1) as c_int;
                        *INTEGER(match_lengths).add(idx) = (m.end - m.start) as c_int;
                    } else {
                        *INTEGER(elt).add(idx) = -1;
                        *INTEGER(match_lengths).add(idx) = -1;
                    }
                }
                (elt, match_lengths)
            } else {
                let elt = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                let match_lengths = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                if elt.is_null() || match_lengths.is_null() {
                    return R_NilValue();
                }
                let _elt_guard = protect(elt);
                let _ml_guard = protect(match_lengths);
                *INTEGER(elt) = -1;
                *INTEGER(match_lengths) = -1;
                (elt, match_lengths)
            };

            if perl {
                set_regexec_perl_attrs(elt, match_lengths);
            } else {
                set_regexpr_attrs(elt, match_lengths);
            }
            SET_VECTOR_ELT(result, i, elt);
        }

        result
    }
}

unsafe fn set_regexpr_attrs(x: SEXP, match_lengths: SEXP) {
    unsafe {
        crate::sexp::attrib_core::setAttrib(x, Rf_install(c"match.length".as_ptr()), match_lengths);
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(c"index.type".as_ptr()),
            Rf_mkString(c"chars".as_ptr()),
        );
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(c"useBytes".as_ptr()),
            Rf_ScalarLogical(TRUE),
        );
    }
}

unsafe fn set_regexec_perl_attrs(x: SEXP, match_lengths: SEXP) {
    unsafe {
        crate::sexp::attrib_core::setAttrib(x, Rf_install(c"match.length".as_ptr()), match_lengths);
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(c"useBytes".as_ptr()),
            Rf_ScalarLogical(TRUE),
        );
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(c"index.type".as_ptr()),
            Rf_mkString(c"chars".as_ptr()),
        );
    }
}

/// R charToRaw(x)
pub unsafe fn do_charToRaw(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        // Upstream raw.c do_charToRaw: requires a character vector of
        // length >= 1; all but the first element are ignored.
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP || XLENGTH(x) == 0 {
            std::panic::panic_any(RError {
                message: "argument must be a character vector of length 1".to_string(),
            });
        }
        let s = elt_to_string(x, 0).as_bytes().to_vec();
        let result = Rf_allocVector3(SEXPTYPE::RAWSXP, s.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let data = (*result).gengc_next_node as *mut u8;
        for (i, &b) in s.iter().enumerate() {
            *data.add(i) = b;
        }
        result
    }
}

/// GNU `rawToChar(x)` copies raw bytes, stripping trailing nuls.
pub unsafe fn do_rawToChar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::RAWSXP {
            std::panic::panic_any(RError {
                message: "argument 'x' must be a raw vector".to_string(),
            });
        }
        let n = XLENGTH(x);
        let data = RAW(x);
        let mut last = -1i32;
        for i in 0..n {
            if *data.add(i as usize) != 0 {
                last = i as i32;
            }
        }
        let nc = last + 1;
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _o = protect(out);
        let ch = crate::sexp::constructors::Rf_mkCharLen(data as *const std::os::raw::c_char, nc);
        SET_STRING_ELT(out, 0, ch);
        out
    }
}

// ---------------------------------------------------------------------------
// do_abs — absolute value
// ---------------------------------------------------------------------------

/// R's `abs(x)` — absolute value of numeric vector.
///
/// Preserves integer/logical inputs as integer vectors and real inputs as
/// real vectors. Non-numeric arguments (and factors, via Math.factor)
/// error like stock Math1.
unsafe fn math_factor_error(call: SEXP, name: &str) -> ! {
    unsafe {
        let method_call = crate::sexp::constructors::Rf_lang2(
            Rf_install(c"Math.factor".as_ptr()),
            CAR(CDR(call)),
        );
        let _guard = protect(method_call);
        crate::mainutils::errors::errorcall_str(
            method_call,
            &format!("'{name}' not meaningful for factors"),
        );
    }
}

fn math_nonnum_error() -> ! {
    std::panic::panic_any(RError {
        message: "non-numeric argument to mathematical function".to_string(),
    })
}

pub unsafe fn do_abs(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut dispatched = R_NilValue();
        if crate::eval::dispatch::DispatchGroup(
            c"Math".as_ptr(),
            call,
            op,
            args,
            rho,
            &mut dispatched,
        ) != 0
        {
            return dispatched;
        }
        let x_arg = CAR(args);
        if x_arg.is_null() {
            return R_NilValue();
        }
        if x_arg == R_NilValue() {
            // stock Math1: NULL is not numeric and errors
            math_nonnum_error();
        }
        if sexp_has_class(x_arg, "factor") {
            math_factor_error(call, "abs");
        }
        let t = TYPEOF(x_arg);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_abs_vec(x_arg);
        }
        if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::LGLSXP {
            math_nonnum_error();
        }
        let n = XLENGTH(x_arg);
        if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = INTEGER(result);
            for i in 0..n {
                let value = *INTEGER(x_arg).add(i as usize);
                *dst.add(i as usize) = if value == NA_INTEGER || value == c_int::MIN {
                    NA_INTEGER
                } else {
                    value.abs()
                };
            }
            return result;
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let v = *REAL(x_arg).add(i as usize);
            *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                v
            } else {
                v.abs()
            };
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_sign — sign of values
// ---------------------------------------------------------------------------

/// R's `sign(x)` — sign of numeric vector (-1, 0, or 1).
///
/// Returns REALSXP. Preserves NA and NaN.
pub unsafe fn do_sign(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        if x_arg.is_null() {
            return R_NilValue();
        }
        if x_arg == R_NilValue() {
            // stock Math1: NULL is not numeric and errors
            math_nonnum_error();
        }
        if sexp_has_class(x_arg, "factor") {
            math_factor_error(call, "sign");
        }
        let t = TYPEOF(x_arg);
        if t == SEXPTYPE::CPLXSXP {
            std::panic::panic_any(RError {
                message: "unimplemented complex function".to_string(),
            });
        }
        if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::LGLSXP {
            math_nonnum_error();
        }
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(i as usize)
            } else {
                let iv = *INTEGER(x_arg).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            };
            *dst.add(i as usize) = if v.is_nan() {
                v // preserve NaN/NA
            } else if v == 0.0 {
                0.0
            } else if v > 0.0 {
                1.0
            } else {
                -1.0
            };
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Complete special functions for libRmath coverage
// ---------------------------------------------------------------------------

/// Helper to apply a scalar function to a numeric vector, preserving NA/NaN.
/// Returns REALSXP.
unsafe fn apply_unary_scalar_fn(x: SEXP, scalar_fn: impl Fn(f64) -> f64) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = scalar_fn(val);
            }
        }
        result
    }
}

/// Helper to apply a binary scalar function to two numeric vectors with recycling.
/// Returns REALSXP.
unsafe fn apply_binary_scalar_fn(x: SEXP, y: SEXP, scalar_fn: impl Fn(f64, f64) -> f64) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
            return R_NilValue();
        }
        let x_len = XLENGTH(x);
        let y_len = XLENGTH(y);
        let n = if x_len == 0 || y_len == 0 {
            0
        } else {
            x_len.max(y_len)
        };
        let tx = TYPEOF(x);
        let ty = TYPEOF(y);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        if x_len == 0 || y_len == 0 {
            if x_len == 0 {
                copy_all_attribs(result, x);
            }
            return result;
        }
        let dst = REAL(result);
        for i in 0..n {
            let xi = if x_len > 0 { i % x_len } else { 0 };
            let yi = if y_len > 0 { i % y_len } else { 0 };
            let val_x = if tx == SEXPTYPE::REALSXP {
                *REAL(x).add(xi as usize)
            } else if tx == SEXPTYPE::INTSXP || tx == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(xi as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            let val_y = if ty == SEXPTYPE::REALSXP {
                *REAL(y).add(yi as usize)
            } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
                let v = *INTEGER(y).add(yi as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val_x.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                || val_y.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = scalar_fn(val_x, val_y);
            }
        }
        if n == x_len {
            copy_all_attribs(result, x);
        } else if n == y_len {
            copy_all_attribs(result, y);
        }
        result
    }
}

/// R's `lgamma(x)` — log of the absolute value of the gamma function.
pub unsafe fn do_lgamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { apply_unary_scalar_fn(CAR(args), crate::special::gamma::lgammafn) }
}

/// R's `gamma(x)` — gamma function.
pub unsafe fn do_gamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { apply_unary_scalar_fn(CAR(args), crate::special::gamma::gammafn) }
}

/// R's `digamma(x)` — digamma (psi) function.
pub unsafe fn do_digamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { apply_unary_scalar_fn(CAR(args), crate::special::polygamma::digamma) }
}

/// R's `trigamma(x)` — trigamma function.
pub unsafe fn do_trigamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { apply_unary_scalar_fn(CAR(args), crate::special::polygamma::trigamma) }
}

/// R's `psigamma(x, deriv)` — polygamma function (deriv-th derivative of psi).
pub unsafe fn do_psigamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let deriv_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || deriv_arg.is_null() || deriv_arg == R_NilValue() {
            return R_NilValue();
        }
        apply_binary_scalar_fn(x, deriv_arg, crate::special::polygamma::psigamma)
    }
}

/// R's `beta(a, b)` — beta function.
pub unsafe fn do_beta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let a = CAR(args);
        let b = CAR(CDR(args));
        if a.is_null() || a == R_NilValue() || b.is_null() || b == R_NilValue() {
            return R_NilValue();
        }
        apply_binary_scalar_fn(a, b, |x, y| {
            crate::special::gamma::gammafn(x) * crate::special::gamma::gammafn(y)
                / crate::special::gamma::gammafn(x + y)
        })
    }
}

/// R's `lbeta(a, b)` — log beta function.
pub unsafe fn do_lbeta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let a = CAR(args);
        let b = CAR(CDR(args));
        if a.is_null() || a == R_NilValue() || b.is_null() || b == R_NilValue() {
            return R_NilValue();
        }
        apply_binary_scalar_fn(a, b, crate::special::lbeta::lbeta)
    }
}

/// R's `choose(n, k)` — binomial coefficient.
pub unsafe fn do_choose(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let k_arg = CAR(CDR(args));
        if n_arg.is_null() || n_arg == R_NilValue() || k_arg.is_null() || k_arg == R_NilValue() {
            return R_NilValue();
        }
        apply_binary_scalar_fn(n_arg, k_arg, crate::special::choose::choose)
    }
}

/// R's `lchoose(n, k)` — log of absolute value of binomial coefficient.
pub unsafe fn do_lchoose(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let k_arg = CAR(CDR(args));
        if n_arg.is_null() || n_arg == R_NilValue() || k_arg.is_null() || k_arg == R_NilValue() {
            return R_NilValue();
        }
        apply_binary_scalar_fn(n_arg, k_arg, crate::special::choose::lchoose)
    }
}

/// R's `factorial(n)` — factorial n!
pub unsafe fn do_factorial(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        apply_unary_scalar_fn(x, |v| crate::special::gamma::gammafn(v + 1.0))
    }
}

/// R's `lfactorial(n)` — log factorial.
pub unsafe fn do_lfactorial(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        apply_unary_scalar_fn(x, |v| crate::special::gamma::lgammafn(v + 1.0))
    }
}

/// R's `besselI(x, nu)` — modified Bessel function of the first kind.
pub unsafe fn do_besselI(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let nu_arg = CAR(CDR(args));
        let expo_arg = CAR(CDR(CDR(args))); // optional: exponential scaling
        if x.is_null() || x == R_NilValue() || nu_arg.is_null() || nu_arg == R_NilValue() {
            return R_NilValue();
        }
        let nu = real_or_default(nu_arg, 0.0);
        let expo = if !expo_arg.is_null() && expo_arg != R_NilValue() {
            let e = real_or_default(expo_arg, 0.0);
            e != 0.0
        } else {
            false
        };
        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) =
                    crate::special::bessel_i::bessel_i(val, nu, if expo { 2.0 } else { 1.0 });
            }
        }
        result
    }
}

/// R's `besselJ(x, nu)` — Bessel function of the first kind.
pub unsafe fn do_besselJ(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Mathlib warnings raised inside bessel_j attribute to this call
        // (upstream resolves it by walking out of the builtin context).
        let _mathlib_call = crate::mainutils::errors::mathlib_warning_call_guard(call);
        let x = CAR(args);
        let nu_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || nu_arg.is_null() || nu_arg == R_NilValue() {
            return R_NilValue();
        }
        let nu = real_or_default(nu_arg, 0.0);
        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        let na_bit = crate::sexp::ffi::R_NA_BIT_PATTERN;
        let mut naflag = false;
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.to_bits() == na_bit {
                *dst.add(i as usize) = NA_REAL;
            } else {
                let b = crate::special::bessel_j::bessel_j(val, nu);
                *dst.add(i as usize) = b;
                if b.is_nan() && !val.is_nan() {
                    naflag = true;
                }
            }
        }
        if naflag {
            crate::mainutils::errors::Rf_warningcall1(call, c"NaNs produced".as_ptr());
        }
        result
    }
}

/// R's `besselK(x, nu)` — modified Bessel function of the second kind.
pub unsafe fn do_besselK(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let nu_arg = CAR(CDR(args));
        let expo_arg = CAR(CDR(CDR(args))); // optional: exponential scaling
        if x.is_null() || x == R_NilValue() || nu_arg.is_null() || nu_arg == R_NilValue() {
            return R_NilValue();
        }
        let nu = real_or_default(nu_arg, 0.0);
        let expo = if !expo_arg.is_null() && expo_arg != R_NilValue() {
            let e = real_or_default(expo_arg, 0.0);
            e != 0.0
        } else {
            false
        };
        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) =
                    crate::special::bessel_k::bessel_k(val, nu, if expo { 2.0 } else { 1.0 });
            }
        }
        result
    }
}

/// R's `besselY(x, nu)` — Bessel function of the second kind.
pub unsafe fn do_besselY(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let nu_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || nu_arg.is_null() || nu_arg == R_NilValue() {
            return R_NilValue();
        }
        let nu = real_or_default(nu_arg, 0.0);
        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = crate::special::bessel_y::bessel_y(val, nu);
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Final additions: commonly used missing functions
// ---------------------------------------------------------------------------

/// R's `simplify2array(x)` — simplify list to array.
pub unsafe fn do_simplify2array(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "simplify2array",
            include_str!("../base_wrappers/simplify2array.R"),
            args,
            rho,
            true,
        )
    }
}

/// R's `match.arg(arg, choices)` — match argument against choices.
pub unsafe fn do_match_arg(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let arg = CAR(args);
        let choices = CAR(CDR(args));
        if arg.is_null() || choices.is_null() || arg == R_NilValue() || choices == R_NilValue() {
            return arg;
        }
        let arg_str = elt_to_string(arg, 0);
        let n = XLENGTH(choices);
        let mut matches = Vec::new();
        for i in 0..n {
            let choice = elt_to_string(choices, i);
            if choice.starts_with(&arg_str) {
                matches.push(choice);
            }
        }
        if matches.len() == 1 {
            Rf_mkString(
                CString::new(matches[0].as_str())
                    .unwrap_or_default()
                    .as_ptr(),
            )
        } else {
            // Upstream match.arg raises with the call so the error renders
            // "Error in match.arg(...) : 'arg' should be one of ...".
            // Upstream appends the (here empty) choice list after "one of ";
            // with zero choices stock R's message ends at "of", and the
            // trailing space would break top-level render matching.
            crate::mainutils::errors::errorcall_str(call, "'arg' should be one of");
        }
    }
}

/// R's `char.expand(input, target)` — expand abbreviations.
pub unsafe fn do_char_expand(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let input = CAR(args);
        let target = CAR(CDR(args));
        let nomatch = CAR(CDR(CDR(args)));
        if input.is_null() || target.is_null() {
            return input;
        }
        let input_str = elt_to_string(input, 0);
        let n = if target == R_NilValue() {
            0
        } else {
            XLENGTH(target)
        };
        let mut matches: Vec<String> = Vec::new();
        for i in 0..n {
            let t = elt_to_string(target, i);
            if t.starts_with(&input_str) {
                matches.push(t);
            }
        }
        if matches.len() == 1 {
            Rf_mkString(CString::new(&matches[0][..]).unwrap_or_default().as_ptr())
        } else if matches.len() > 1 {
            Rf_allocVector3(SEXPTYPE::STRSXP, 0)
        } else if !nomatch.is_null() && nomatch != R_NilValue() && nomatch != R_MissingArg() {
            let out = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
            if out.is_null() {
                return R_NilValue();
            }
            SET_STRING_ELT(out, 0, crate::sexp::globals::R_NaString());
            out
        } else {
            // Upstream char.expand ends with `eval(nomatch)` where nomatch
            // defaults to stop("no match"); stock R attributes the error to
            // that eval call.
            let sym = |name: &str| {
                crate::sexp::symbol::Rf_install(
                    std::ffi::CString::new(name).unwrap_or_default().as_ptr(),
                )
            };
            let eval_call = crate::sexp::constructors::Rf_lang2(sym("eval"), sym("nomatch"));
            crate::mainutils::errors::errorcall_str(eval_call, "no match");
        }
    }
}

/// R's `type.convert(x, ...)` — convert to appropriate type.
pub unsafe fn do_type_convert(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return x;
        }
        // Try integer first
        let n = XLENGTH(x);
        let first = elt_to_string(x, 0);
        if first.parse::<i64>().is_ok() {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
            if result.is_null() {
                return x;
            }
            let _p = protect(result);
            for i in 0..n {
                let s = elt_to_string(x, i);
                *INTEGER(result).add(i as usize) = s.parse::<i64>().unwrap_or(0) as c_int;
            }
            result
        } else if crate::mainutils::coerce::parse_double_str(&first).is_some() {
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
            if result.is_null() {
                return x;
            }
            let _p = protect(result);
            for i in 0..n {
                let s = elt_to_string(x, i);
                *REAL(result).add(i as usize) =
                    crate::mainutils::coerce::parse_double_str(&s).unwrap_or(NA_REAL);
            }
            result
        } else {
            x // Keep as character
        }
    }
}

/// R's `as.environment(x)` — convert to environment.
pub unsafe fn do_as_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }
        if TYPEOF(x) == SEXPTYPE::ENVSXP {
            return x;
        }
        if TYPEOF(x) == SEXPTYPE::INTSXP || TYPEOF(x) == SEXPTYPE::REALSXP {
            let pos = if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(x)
            } else {
                *REAL(x) as c_int
            };
            return search_env_from_position(pos);
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP {
            let name = if XLENGTH(x) == 0 {
                "NA".to_string()
            } else {
                elt_to_string(x, 0)
            };
            return search_env_from_name(&name);
        }
        if TYPEOF(x) == SEXPTYPE::VECSXP || TYPEOF(x) == SEXPTYPE::LISTSXP {
            // Upstream as.environment on a named list/pairlist: bindings
            // from the elements, parent = emptyenv() (whisker's partials
            // path relies on this).
            let n = XLENGTH(x);
            let names =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            let names_ok = !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && XLENGTH(names) == n
                && (0..n).all(|i| {
                    let si = crate::sexp::accessors::STRING_ELT(names, i as i64);
                    !si.is_null() && si != crate::sexp::globals::R_NaString()
                });
            if !names_ok {
                std::panic::panic_any(RError {
                    message: "names(x) must be a character vector of the same length as x"
                        .to_string(),
                });
            }
            let env = crate::sexp::envir::R_NewHashedEnv(crate::sexp::globals::R_EmptyEnv(), 0);
            let _env_guard = protect(env);
            for i in 0..n {
                let value = if TYPEOF(x) == SEXPTYPE::VECSXP {
                    crate::sexp::accessors::VECTOR_ELT(x, i as i64)
                } else {
                    // pairlist walk: CAR of the i-th cons cell
                    let mut cell = x;
                    for _ in 0..i {
                        cell = CDR(cell);
                    }
                    CAR(cell)
                };
                let name = elt_to_string(names, i);
                let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
                crate::sexp::envir::defineVar(sym, value, env);
            }
            return env;
        }
        std::panic::panic_any(RError {
            message: "invalid object for as.environment".to_string(),
        });
    }
}

/// R's `pos.to.env(pos)` — map a search path position to an environment.
pub unsafe fn do_pos_to_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pos = integer_arg_by_name_or_position(args, "pos", 0).unwrap_or(NA_INTEGER);
        search_env_from_position(pos)
    }
}

unsafe fn search_env_from_position(pos: c_int) -> SEXP {
    unsafe {
        if pos > 0
            && let Some((_, env)) = search_path_entries().get((pos - 1) as usize)
        {
            return *env;
        }
        std::panic::panic_any(RError {
            message: "invalid 'pos' argument".to_string(),
        });
    }
}

unsafe fn search_env_from_name(name: &str) -> SEXP {
    for (label, env) in unsafe { search_path_entries() } {
        if label == name || (name == "base" && label == "package:base") {
            return env;
        }
    }
    std::panic::panic_any(RError {
        message: format!("no item called \"{name}\" on the search list"),
    });
}

unsafe fn search_path_len() -> c_int {
    unsafe { search_path_entries().len() as c_int }
}

pub(crate) unsafe fn search_path_entries() -> Vec<(String, SEXP)> {
    unsafe {
        let global = crate::sexp::globals::R_GlobalEnv();
        let base = crate::sexp::globals::R_BaseEnv();
        if global.is_null() || base.is_null() {
            return Vec::new();
        }

        let mut entries = vec![(".GlobalEnv".to_string(), global)];
        let mut env = crate::sexp::accessors::ENCLOS(global);
        while !env.is_null() && env != base {
            entries.push((search_env_label(env), env));
            env = crate::sexp::accessors::ENCLOS(env);
        }
        entries.push(("package:base".to_string(), base));
        entries
    }
}

unsafe fn search_env_label(env: SEXP) -> String {
    unsafe {
        let name = crate::sexp::attrib_core::getAttrib(env, Rf_install(c"name".as_ptr()));
        if TYPEOF(name) == SEXPTYPE::STRSXP && XLENGTH(name) > 0 {
            let value = STRING_ELT(name, 0);
            if !value.is_null() && value != R_NilValue() {
                return CStr::from_ptr(CHAR(value)).to_string_lossy().into_owned();
            }
        }
        "(unknown)".to_string()
    }
}

/// R's `searchpaths()` — filesystem/search labels for entries on the search path.
pub unsafe fn do_searchpaths(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let entries = search_path_entries();
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, entries.len() as R_xlen_t);
        for (i, (label, _)) in entries.iter().enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(label.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

/// R's `sort.list(x, partial, na.last, decreasing, method)` — indices for sorting.
pub unsafe fn do_sort_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let decreasing = sort_logical_arg(args, &["decreasing"], 3).unwrap_or(false);
        let na_placement = order_na_placement(args, 2);
        let mut indices = ordered_atomic_indices(x, decreasing, na_placement);
        if na_placement == SortNaPlacement::Remove {
            let compressed_positions = nonmissing_compressed_positions(x);
            for index in &mut indices {
                *index = compressed_positions[*index as usize];
            }
        }

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, indices.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for (i, idx) in indices.iter().enumerate() {
            *INTEGER(result).add(i) = (*idx + 1) as c_int; // 1-indexed
        }
        result
    }
}

fn nonmissing_compressed_positions(x: SEXP) -> Vec<R_xlen_t> {
    unsafe {
        let n = XLENGTH(x);
        let mut positions = vec![0; n as usize];
        let mut next = 0;
        for i in 0..n {
            let missing = match TYPEOF(x) {
                t if t == SEXPTYPE::STRSXP => charsxp_is_na(STRING_ELT(x, i)),
                t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                    *INTEGER(x).add(i as usize) == NA_INTEGER
                }
                t if t == SEXPTYPE::REALSXP => ISNAN(*REAL(x).add(i as usize)),
                _ => ISNAN(elt_real_safe(x, i)),
            };
            if !missing {
                positions[i as usize] = next;
                next += 1;
            }
        }
        positions
    }
}

/// R's `outer(X, Y, FUN)` — outer product (enhanced).
pub unsafe fn do_outer_enhanced(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let fun = CAR(CDR(CDR(args)));
        if x.is_null() || y.is_null() {
            return R_NilValue();
        }
        let nx = XLENGTH(x);
        let ny = XLENGTH(y);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, nx * ny);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        // Default: multiplication
        if nx > 0 && ny > 0 {
            let dst = REAL(result);
            for i in 0..nx {
                let xi = elt_real_safe(x, i);
                for j in 0..ny {
                    let yj = elt_real_safe(y, j);
                    *dst.add((i * ny + j) as usize) = xi * yj;
                }
            }
        }

        // Set dim attribute
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            *INTEGER(dim) = nx as c_int;
            *INTEGER(dim).add(1) = ny as c_int;
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"dim".as_ptr()), dim);
        }
        result
    }
}

/// R's `match.fun(FUN)` — match a function argument.
pub unsafe fn do_match_fun(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }
        if TYPEOF(x) == SEXPTYPE::CLOSXP
            || TYPEOF(x) == SEXPTYPE::BUILTINSXP
            || TYPEOF(x) == SEXPTYPE::SPECIALSXP
        {
            return x;
        }
        // If it's a symbol, look it up
        if TYPEOF(x) == SEXPTYPE::SYMSXP {
            let val = crate::sexp::envir::R_findVar(x, _rho);
            if !val.is_null()
                && (TYPEOF(val) == SEXPTYPE::CLOSXP
                    || TYPEOF(val) == SEXPTYPE::BUILTINSXP
                    || TYPEOF(val) == SEXPTYPE::SPECIALSXP)
            {
                return val;
            }
        }
        x
    }
}
