/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1998--2025  The R Core Team.
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 *
 *  Ported from r-source/src/library/stats/src/fourier.c
 *
 *  These are the R interface routines to the plain FFT code
 *  fft_factor() & fft_work() in fft.c.
 */

use std::os::raw::{c_double, c_int};

use num::complex::Complex64;

use crate::attrib_core::{R_DimSymbol, getAttrib};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::protect::protect;

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    unsafe { crate::main::coerce::coerceVector(x, type_) }
}

unsafe fn duplicate(x: SEXP) -> SEXP {
    unsafe { crate::main::duplicate::duplicate(x) }
}

/// Pure-Rust mixed-radix discrete Fourier transform.
///
/// Composite lengths use Cooley--Tukey decomposition; prime leaves use the
/// direct transform.  This keeps arbitrary-length GNU R semantics without a
/// BLAS/Fortran dependency (notably on WASM and Android), while avoiding the
/// quadratic path for the overwhelmingly common composite sizes.
fn transform(values: &[Complex64], inverse: bool) -> Vec<Complex64> {
    let n = values.len();
    if n <= 1 {
        return values.to_vec();
    }

    let mut factor = n;
    let mut candidate = 2usize;
    while candidate <= n / candidate {
        if n.is_multiple_of(candidate) {
            factor = candidate;
            break;
        }
        candidate += 1;
    }
    let sign = if inverse { 1.0 } else { -1.0 };

    if factor == n {
        return (0..n)
            .map(|k| {
                let step =
                    Complex64::from_polar(1.0, sign * std::f64::consts::TAU * k as f64 / n as f64);
                let mut twiddle = Complex64::new(1.0, 0.0);
                let mut sum = Complex64::new(0.0, 0.0);
                for &value in values {
                    sum += value * twiddle;
                    twiddle *= step;
                }
                sum
            })
            .collect();
    }

    let inner_len = n / factor;
    let inner: Vec<Vec<Complex64>> = (0..factor)
        .map(|residue| {
            let lane: Vec<_> = (0..inner_len)
                .map(|index| values[index * factor + residue])
                .collect();
            transform(&lane, inverse)
        })
        .collect();

    let mut output = vec![Complex64::new(0.0, 0.0); n];
    for k in 0..n {
        let inner_k = k % inner_len;
        let step = Complex64::from_polar(1.0, sign * std::f64::consts::TAU * k as f64 / n as f64);
        let mut twiddle = Complex64::new(1.0, 0.0);
        for lane in &inner {
            output[k] += lane[inner_k] * twiddle;
            twiddle *= step;
        }
    }
    output
}

#[cfg(test)]
mod transform_tests {
    use super::*;

    fn close(actual: Complex64, expected: Complex64) {
        assert!(
            (actual - expected).norm() < 1e-10,
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn forward_matches_gnu_r_oracle_and_inverse_is_unscaled() {
        let input: Vec<_> = (1..=4)
            .map(|value| Complex64::new(value as f64, 0.0))
            .collect();
        let forward = transform(&input, false);
        let oracle = [
            Complex64::new(10.0, 0.0),
            Complex64::new(-2.0, 2.0),
            Complex64::new(-2.0, 0.0),
            Complex64::new(-2.0, -2.0),
        ];
        for (actual, expected) in forward.iter().copied().zip(oracle) {
            close(actual, expected);
        }
        for (actual, expected) in transform(&forward, true).into_iter().zip(input) {
            close(actual, expected * 4.0);
        }
    }

    #[test]
    fn prime_length_round_trip_matches_gnu_r_scaling() {
        let input: Vec<_> = (0..7)
            .map(|index| Complex64::new(index as f64 - 2.0, index as f64 / 3.0))
            .collect();
        let forward = transform(&input, false);
        for (actual, expected) in transform(&forward, true).into_iter().zip(input) {
            close(actual, expected * 7.0);
        }
    }

    #[test]
    fn sexp_front_end_transforms_real_vectors() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let input = Rf_allocVector(SEXPTYPE::REALSXP.0, 4);
            let _input_guard = protect(input);
            for (index, value) in [1.0, 2.0, 3.0, 4.0].into_iter().enumerate() {
                *REAL(input).add(index) = value;
            }
            let inverse = Rf_ScalarLogical(0);
            let _inverse_guard = protect(inverse);
            let result = fft(input, inverse);
            assert_eq!(LENGTH(result), 4);
            let expected = [
                Rcomplex { r: 10.0, i: 0.0 },
                Rcomplex { r: -2.0, i: 2.0 },
                Rcomplex { r: -2.0, i: 0.0 },
                Rcomplex { r: -2.0, i: -2.0 },
            ];
            for (index, expected) in expected.into_iter().enumerate() {
                close(
                    Complex64::new(
                        (*COMPLEX(result).add(index)).r,
                        (*COMPLEX(result).add(index)).i,
                    ),
                    Complex64::new(expected.r, expected.i),
                );
            }
        }
    }

    #[test]
    fn strided_transform_matches_gnu_r_matrix_oracle() {
        let mut values: Vec<_> = (1..=4)
            .map(|value| Rcomplex {
                r: value as f64,
                i: 0.0,
            })
            .collect();
        unsafe {
            transform_line(values.as_mut_ptr(), 2, 1, false);
            transform_line(values.as_mut_ptr().add(2), 2, 1, false);
            transform_line(values.as_mut_ptr(), 2, 2, false);
            transform_line(values.as_mut_ptr().add(1), 2, 2, false);
        }
        for (actual, expected) in values.into_iter().zip([10.0, -2.0, -4.0, 0.0]) {
            close(
                Complex64::new(actual.r, actual.i),
                Complex64::new(expected, 0.0),
            );
        }
    }
}

unsafe fn transform_line(base: *mut Rcomplex, len: usize, stride: usize, inverse: bool) {
    unsafe {
        let input: Vec<_> = (0..len)
            .map(|index| {
                let value = *base.add(index * stride);
                Complex64::new(value.r, value.i)
            })
            .collect();
        for (index, value) in transform(&input, inverse).into_iter().enumerate() {
            *base.add(index * stride) = Rcomplex {
                r: value.re,
                i: value.im,
            };
        }
    }
}

/// true if namedness > 0 (potentially shared).
unsafe fn MAYBE_REFERENCED(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    unsafe { crate::sexp::accessors::NAMED(x) as c_int }
}

unsafe fn as_integer(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    unsafe {
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP {
            return *INTEGER(x);
        }
        if t == SEXPTYPE::REALSXP {
            let v = *REAL(x);
            if v.is_nan() || v < c_int::MIN as f64 || v > c_int::MAX as f64 {
                return NA_INTEGER;
            }
            return v as c_int;
        }
        if t == SEXPTYPE::LGLSXP {
            return *INTEGER(x);
        }
        NA_INTEGER
    }
}

unsafe fn as_logical(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    unsafe {
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP {
            return *INTEGER(x);
        }
        if t == SEXPTYPE::INTSXP {
            return *INTEGER(x);
        }
        NA_INTEGER
    }
}

/* Fourier Transform for Univariate Spatial and Time Series */

pub unsafe fn fft(z: SEXP, inverse: SEXP) -> SEXP {
    use crate::main::errors::Rf_error;

    let mut z = z;

    unsafe {
        match TYPEOF(z) as c_int {
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP || t == SEXPTYPE::REALSXP => {
                z = coerceVector(z, SEXPTYPE::CPLXSXP.as_c_int());
            }
            t if t == SEXPTYPE::CPLXSXP => {
                if MAYBE_REFERENCED(z) != 0 {
                    z = duplicate(z);
                }
            }
            _ => {
                Rf_error(b"non-numeric argument\0".as_ptr() as *const libc::c_char);
            }
        }
        let _z_guard = protect(z);

        let inverse = as_logical(inverse) != NA_INTEGER && as_logical(inverse) != 0;

        if LENGTH(z) > 1 {
            let d = getAttrib(z, R_DimSymbol());
            if d.is_null() || d == R_NilValue() {
                /* temporal transform */
                transform_line(COMPLEX(z), LENGTH(z) as usize, 1, inverse);
            } else {
                /* spatial transform */
                let ndims = LENGTH(d);
                let total = LENGTH(z) as usize;
                let mut stride = 1usize;
                for i in 0..(ndims as usize) {
                    let len = *INTEGER(d).add(i) as usize;
                    if len > 1 {
                        let block_len = stride * len;
                        for block in 0..(total / block_len) {
                            for lane in 0..stride {
                                transform_line(
                                    COMPLEX(z).add(block * block_len + lane),
                                    len,
                                    stride,
                                    inverse,
                                );
                            }
                        }
                    }
                    stride *= len;
                }
            }
        }
    }
    z
}

/// Evaluator adapter for the public `fft(x, inverse = FALSE)` closure.
///
/// GNU R implements this small front end in R and delegates to the stats
/// native routine.  The port currently exposes package functions directly as
/// evaluator builtins, so keep the default here while reusing the translated,
/// allocation-safe FFT backend above.
pub unsafe fn do_fft(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let z = CAR(args);
        if z.is_null() || z == R_NilValue() || z == R_MissingArg() {
            crate::main::errors::errorcall_str(call, "argument \"z\" is missing, with no default");
        }
        let inverse = CADR(args);
        if inverse.is_null() || inverse == R_NilValue() || inverse == R_MissingArg() {
            let default_inverse = Rf_ScalarLogical(0);
            let _default_guard = protect(default_inverse);
            fft(z, default_inverse)
        } else {
            fft(z, inverse)
        }
    }
}

/* Fourier Transform for Vector-Valued ("multivariate") Series */

pub unsafe fn mvfft(z: SEXP, inverse: SEXP) -> SEXP {
    use crate::main::errors::Rf_error;

    let mut z = z;
    unsafe {
        let d = getAttrib(z, R_DimSymbol());
        if d.is_null() || d == R_NilValue() || LENGTH(d) != 2 {
            Rf_error(
                b"vector-valued (multivariate) series required\0".as_ptr() as *const libc::c_char
            );
        }
        let n = *INTEGER(d);
        let p = *INTEGER(d).add(1);

        match TYPEOF(z) as c_int {
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP || t == SEXPTYPE::REALSXP => {
                z = coerceVector(z, SEXPTYPE::CPLXSXP.as_c_int());
            }
            t if t == SEXPTYPE::CPLXSXP => {
                if MAYBE_REFERENCED(z) != 0 {
                    z = duplicate(z);
                }
            }
            _ => {
                Rf_error(b"non-numeric argument\0".as_ptr() as *const libc::c_char);
            }
        }
        let _z_guard = protect(z);

        let inverse = as_logical(inverse) != NA_INTEGER && as_logical(inverse) != 0;

        if n > 1 {
            for i in 0..(p as usize) {
                let base = COMPLEX(z).add(i * n as usize);
                transform_line(base, n as usize, 1, inverse);
            }
        }
    }
    z
}

/// Evaluator adapter for `mvfft(x, inverse = FALSE)`.
pub unsafe fn do_mvfft(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let z = CAR(args);
        if z.is_null() || z == R_NilValue() || z == R_MissingArg() {
            crate::main::errors::errorcall_str(call, "argument \"z\" is missing, with no default");
        }
        let inverse = CADR(args);
        if inverse.is_null() || inverse == R_NilValue() || inverse == R_MissingArg() {
            let default_inverse = Rf_ScalarLogical(0);
            let _default_guard = protect(default_inverse);
            mvfft(z, default_inverse)
        } else {
            mvfft(z, inverse)
        }
    }
}

fn ok_n(mut n: c_int, f: &[c_int]) -> bool {
    for &factor in f {
        loop {
            if n % factor != 0 {
                break;
            }
            n = n / factor;
            if n == 1 {
                return true;
            }
        }
    }
    n == 1
}

fn ok_n_64(mut n: u64, f: &[c_int]) -> bool {
    for &factor in f {
        loop {
            let factor = factor as u64;
            if n % factor != 0 {
                break;
            }
            n = n / factor;
            if n == 1 {
                return true;
            }
        }
    }
    n == 1
}

fn nextn0(mut n: c_int, f: &[c_int]) -> c_int {
    loop {
        if ok_n(n, f) {
            break;
        }
        if n >= c_int::MAX {
            break;
        }
        n += 1;
    }
    if n >= c_int::MAX {
        unsafe {
            crate::main::errors::Rf_warning(
                b"nextn() found no solution < INT_MAX (the maximal integer)\0".as_ptr()
                    as *const libc::c_char,
            );
        }
        return NA_INTEGER;
    }
    n
}

fn nextn0_64(mut n: u64, f: &[c_int]) -> u64 {
    loop {
        if ok_n_64(n, f) {
            break;
        }
        if n >= u64::MAX {
            break;
        }
        n += 1;
    }
    if n >= u64::MAX {
        unsafe {
            crate::main::errors::Rf_warning(
                b"nextn<64>() found no solution < UINT64_MAX\0".as_ptr() as *const libc::c_char,
            );
        }
        return 0;
    }
    n
}

pub unsafe fn nextn(mut n: SEXP, f: SEXP) -> SEXP {
    use crate::main::errors::Rf_error;

    unsafe {
        if TYPEOF(n) == SEXPTYPE::NILSXP {
            return Rf_allocVector(SEXPTYPE::INTSXP, 0);
        }

        let mut f = f;
        if TYPEOF(f) != SEXPTYPE::INTSXP {
            f = coerceVector(f, SEXPTYPE::INTSXP.as_c_int());
        }
        let _f_guard = protect(f);
        let nf = LENGTH(f);

        /* check the factors */
        if nf == 0 {
            Rf_error(b"no factors\0".as_ptr() as *const libc::c_char);
        }
        if nf < 0 {
            Rf_error(b"too many factors\0".as_ptr() as *const libc::c_char);
        }
        let factors = std::slice::from_raw_parts(INTEGER(f) as *const c_int, nf as usize);
        for &factor in factors {
            if factor == NA_INTEGER || factor <= 1 {
                Rf_error(b"invalid factors\0".as_ptr() as *const libc::c_char);
            }
        }

        let mut use_int = TYPEOF(n) == SEXPTYPE::INTSXP;
        if !use_int && TYPEOF(n) != SEXPTYPE::REALSXP {
            Rf_error(
                b"'n' must have typeof(.) \"integer\" or \"double\"\0".as_ptr()
                    as *const libc::c_char,
            );
        }
        let nn = XLENGTH(n);
        let mut _n_guard = protect(n);
        if !use_int && nn > 0 {
            let d_n = REAL(n);
            let n_values = std::slice::from_raw_parts(d_n as *const c_double, nn as usize);
            let mut n_max: c_double = -1.0;
            for &value in n_values {
                if !ISNAN(value) && value > n_max {
                    n_max = value;
                }
            }
            if n_max <= (c_int::MAX as c_double) / (factors[0] as c_double) {
                use_int = true;
                let n_new = coerceVector(n, SEXPTYPE::INTSXP.as_c_int());
                n = n_new;
                _n_guard = protect(n);
            }
        }

        let ans_type = if use_int {
            SEXPTYPE::INTSXP.as_c_int()
        } else {
            SEXPTYPE::REALSXP.as_c_int()
        };
        let ans = Rf_allocVector(ans_type, nn as c_int);
        let _ans_guard = protect(ans);

        if nn == 0 {
            return ans;
        }

        if use_int {
            let n_ = INTEGER(n);
            let n_values = std::slice::from_raw_parts(n_ as *const c_int, nn as usize);
            let r = INTEGER(ans);
            for (i, &value) in n_values.iter().enumerate() {
                if value == NA_INTEGER {
                    *r.add(i) = NA_INTEGER;
                } else if value <= 1 {
                    *r.add(i) = 1;
                } else {
                    *r.add(i) = nextn0(value, factors);
                }
            }
        } else {
            let n_ = REAL(n);
            let n_values = std::slice::from_raw_parts(n_ as *const c_double, nn as usize);
            let r = REAL(ans);
            for (i, &value) in n_values.iter().enumerate() {
                if ISNAN(value) {
                    *r.add(i) = NA_REAL;
                } else if value <= 1.0 {
                    *r.add(i) = 1.0;
                } else {
                    let max_dbl_int: u64 = 9007199254740992; // 2^53
                    let n_n = nextn0_64(value as u64, factors);
                    if n_n > max_dbl_int {
                        crate::main::errors::Rf_warning(
                            b"nextn() may not be exactly representable in R (as \"double\")\0"
                                .as_ptr() as *const libc::c_char,
                        );
                    }
                    *r.add(i) = n_n as c_double;
                }
            }
        }

        ans
    }
}
