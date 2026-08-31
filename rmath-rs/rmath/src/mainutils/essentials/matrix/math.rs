//! Elementwise math: %in%, real_math1 table, sinpi/cospi family, trigonometric builtins — extracted verbatim from the former single-file module.
use super::*;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;
use std::path::Path;

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
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use crate::sexp::attrib_core::{R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol};

/// R's `%in%` operator — match operator.
pub unsafe fn do_in_operator(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let table = CAR(CDR(args));

        if x.is_null() || x == R_NilValue() || table.is_null() || table == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = LOGICAL(result);

        for i in 0..n {
            let elem = elt_to_string(x, i);
            let table_len = XLENGTH(table);
            let mut found = false;
            for j in 0..table_len {
                let tbl_elem = elt_to_string(table, j);
                if elem == tbl_elem {
                    found = true;
                    break;
                }
            }
            *dst.add(i as usize) = if found { TRUE } else { FALSE };
        }
        result
    }
}

pub unsafe fn real_math1(args: SEXP, f: impl Fn(f64) -> f64) -> SEXP {
    unsafe {
        let x = CAR(args);
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

            *dst.add(i as usize) = if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                NA_REAL
            } else {
                f(val)
            };
        }
        result
    }
}

pub fn sinpi_value(x: f64) -> f64 {
    if x.is_finite() && x.fract() == 0.0 {
        0.0
    } else {
        (std::f64::consts::PI * x).sin()
    }
}

pub fn cospi_value(x: f64) -> f64 {
    if x.is_finite() && x.fract() == 0.0 {
        if (x as i64).rem_euclid(2) == 0 {
            1.0
        } else {
            -1.0
        }
    } else if x.is_finite() && (x - 0.5).fract() == 0.0 {
        0.0
    } else {
        (std::f64::consts::PI * x).cos()
    }
}

/// R's `expm1(x)` — accurate exp(x)-1.
pub unsafe fn do_expm1(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::exp_m1) }
}

/// R's `log1p(x)` — accurate log(1+x).
pub unsafe fn do_log1p(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::ln_1p) }
}

/// R's `acosh(x)` — inverse hyperbolic cosine.
pub unsafe fn do_acosh(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::acosh) }
}

/// R's `asinh(x)` — inverse hyperbolic sine.
pub unsafe fn do_asinh(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::asinh) }
}

/// R's `atanh(x)` — inverse hyperbolic tangent.
pub unsafe fn do_atanh(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::atanh) }
}

/// R's `sinpi(x)` — sin(pi*x), exact at integer arguments.
pub unsafe fn do_sinpi(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, sinpi_value) }
}

/// R's `cospi(x)` — cos(pi*x), exact at integer and half-integer arguments.
pub unsafe fn do_cospi(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, cospi_value) }
}

/// R's `tanpi(x)` — tan(pi*x), based on the exact sinpi/cospi helpers.
pub unsafe fn do_tanpi(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        real_math1(args, |x| {
            if x.is_finite() && x.fract() == 0.0 {
                return 0.0;
            }
            if x.is_finite() {
                let cycle = x.rem_euclid(1.0);
                if cycle == 0.25 {
                    return 1.0;
                }
                if cycle == 0.75 {
                    return -1.0;
                }
            }
            let cos = cospi_value(x);
            if cos == 0.0 {
                f64::NAN
            } else {
                sinpi_value(x) / cos
            }
        })
    }
}

/// R's `sin(x)` — sine function.
pub unsafe fn do_sin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_sin,
            );
        }
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
                *dst.add(i as usize) = val.sin();
            }
        }
        result
    }
}

/// R's `cos(x)` — cosine function.
pub unsafe fn do_cos(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_cos,
            );
        }
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
                *dst.add(i as usize) = val.cos();
            }
        }
        result
    }
}

/// R's `tan(x)` — tangent function.
pub unsafe fn do_tan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_tan,
            );
        }
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
                *dst.add(i as usize) = val.tan();
            }
        }
        result
    }
}

/// R's `asin(x)` — arc sine function.
pub unsafe fn do_asin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
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
                *dst.add(i as usize) = val.asin();
            }
        }
        result
    }
}

/// R's `acos(x)` — arc cosine function.
pub unsafe fn do_acos(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
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
                *dst.add(i as usize) = val.acos();
            }
        }
        result
    }
}

/// R's `atan(x)` — arc tangent function.
pub unsafe fn do_atan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
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
                *dst.add(i as usize) = val.atan();
            }
        }
        result
    }
}

/// R's `atan2(y, x)` — two-argument arc tangent function.
pub unsafe fn do_atan2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let y = CAR(args);
        let x = CAR(CDR(args));

        if y.is_null() || y == R_NilValue() || x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(y).max(XLENGTH(x));
        let ty = TYPEOF(y);
        let tx = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let y_len = XLENGTH(y);
            let x_len = XLENGTH(x);
            let yi = if y_len > 0 { i % y_len } else { 0 };
            let xi = if x_len > 0 { i % x_len } else { 0 };

            let val_y = if ty == SEXPTYPE::REALSXP {
                *REAL(y).add(yi as usize)
            } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
                let v = *INTEGER(y).add(yi as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            let val_x = if tx == SEXPTYPE::REALSXP {
                *REAL(x).add(xi as usize)
            } else if tx == SEXPTYPE::INTSXP || tx == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(xi as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val_y.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                || val_x.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val_y.atan2(val_x);
            }
        }
        result
    }
}
