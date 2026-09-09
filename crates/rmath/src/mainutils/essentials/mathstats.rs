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

/// R rawToChar(x)
pub unsafe fn do_rawToChar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        // Upstream raw.c do_rawToChar: error() unless RAWSXP.
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::RAWSXP {
            std::panic::panic_any(RError {
                message: "argument 'x' must be a raw vector".to_string(),
            });
        }
        let n = XLENGTH(x);
        let data = (*x).gengc_next_node as *const u8;
        let s = String::from_utf8_lossy(std::slice::from_raw_parts(data, n as usize));
        Rf_mkString(CString::new(s.as_ref()).unwrap_or_default().as_ptr())
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
