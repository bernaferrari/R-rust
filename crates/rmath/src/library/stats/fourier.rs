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
 *  These are the R interface routines for the Fourier transform kernel.
 */

use std::os::raw::{c_double, c_int};

use num::complex::Complex64;

use crate::attrib_core::{R_DimSymbol, getAttrib};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::memory::with_arena;
use crate::sexp::protect::protect;
use super::fft::transform;

fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::main::coerce::coerceVector(x, type_)
}

fn duplicate(x: SEXP) -> SEXP {
    crate::mainutils::seq::Rf_duplicate(x)
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
                SET_REAL_ELT(input, index as i32, value);
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
    fn fft_workspace_budget_error_releases_reservation_and_recovers() {
        let mut session = crate::sexp::session::RSession::new();
        unsafe {
            let input = Rf_allocVector(SEXPTYPE::CPLXSXP.0, 4);
            let _input_guard = protect(input);
            for (index, value) in [1.0, 2.0, 3.0, 4.0].into_iter().enumerate() {
                (*COMPLEX(input).add(index)).r = value;
                (*COMPLEX(input).add(index)).i = 0.0;
            }
            let inverse = Rf_ScalarLogical(0);
            let _inverse_guard = protect(inverse);
            let retained_bytes = session
                .with_arena(|arena| arena.total_bytes_allocated())
                .unwrap();
            let workspace = fft_workspace_bytes(4).expect("small FFT workspace");
            session.set_arena_budget(crate::sexp::memory::ArenaBudget::new(
                retained_bytes + workspace - 1,
                0,
            ));
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                fft(input, inverse);
            }))
            .expect_err("FFT workspace limit must be recoverable");
            let error = failure.downcast_ref::<crate::sexp::context::RError>();
            assert!(
                error.is_some_and(|error| error.message.contains("FFT workspace")),
                "expected an FFT workspace error, got {error:?}"
            );

            session.set_arena_budget(crate::sexp::memory::ArenaBudget::unlimited());
            let result = fft(input, inverse);
            assert_eq!(LENGTH(result), 4);
        }
    }

    #[test]
    fn low_budget_error_reporting_preserves_the_original_error() {
        let mut session = crate::sexp::session::RSession::new();
        let retained_bytes = session
            .with_arena(|arena| arena.total_bytes_allocated())
            .unwrap();
        // The error condition itself needs a small, fixed amount of arena
        // space. Leave that room while still exercising the low-budget path.
        session.set_arena_budget(crate::sexp::memory::ArenaBudget::new(
            retained_bytes + 255,
            0,
        ));
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            crate::mainutils::errors::Rf_error_cstr(c"direct low-budget sentinel");
        }))
        .expect_err("Rf_error must remain an R error under a low memory limit");
        assert_eq!(
            failure
                .downcast_ref::<crate::sexp::context::RError>()
                .map(|error| error.message.as_str()),
            Some("direct low-budget sentinel")
        );
    }

    #[test]
    fn strided_transform_matches_gnu_r_matrix_oracle() {
        let _session = crate::sexp::session::RSession::new();
        let mut values: Vec<_> = (1..=4)
            .map(|value| Rcomplex {
                r: value as f64,
                i: 0.0,
            })
            .collect();
        transform_line(&mut values, 0, 2, 1, false);
        transform_line(&mut values, 2, 2, 1, false);
        transform_line(&mut values, 0, 2, 2, false);
        transform_line(&mut values, 1, 2, 2, false);
        for (actual, expected) in values.into_iter().zip([10.0, -2.0, -4.0, 0.0]) {
            close(
                Complex64::new(actual.r, actual.i),
                Complex64::new(expected, 0.0),
            );
        }
    }
}

fn fft_workspace_bytes(len: usize) -> Option<usize> {
    // A transform can retain the input lane, recursive inner results, and
    // the output simultaneously. Four complex buffers is conservative for
    // the recursive mixed-radix implementation and catches size overflow
    // before any Vec allocation.
    len.checked_mul(std::mem::size_of::<Complex64>())?
        .checked_mul(4)
}

/// Transform the `len` cells `buf[start + k * stride]` in place.
fn transform_line(
    buf: &mut [Rcomplex],
    start: usize,
    len: usize,
    stride: usize,
    inverse: bool,
) -> bool {
    let Some(workspace_bytes) = fft_workspace_bytes(len) else {
        return false;
    };
    // SAFETY: the closure only charges the arena's transient ledger; it does
    // not reenter R or reach the arena through another alias.
    let reservation = unsafe { with_arena(|arena| arena.try_reserve_transient(workspace_bytes)) };
    let Some(_reservation) = reservation else {
        return false;
    };
    let input: Vec<_> = (0..len)
        .map(|index| {
            let value = buf[start + index * stride];
            Complex64::new(value.r, value.i)
        })
        .collect();
    for (index, value) in transform(&input, inverse).into_iter().enumerate() {
        buf[start + index * stride] = Rcomplex {
            r: value.re,
            i: value.im,
        };
    }
    true
}

/// Copy a CPLXSXP payload through checked element reads.
fn complex_vec(z: SEXP) -> Vec<Rcomplex> {
    (0..LENGTH(z)).map(|i| COMPLEX_ELT(z, i)).collect()
}

/// Store `values` into a CPLXSXP through checked element writes.
fn store_complex(z: SEXP, values: &[Rcomplex]) {
    for (i, &v) in values.iter().enumerate() {
        SET_COMPLEX_ELT(z, i as c_int, v);
    }
}

/// true if namedness > 0 (potentially shared).
fn MAYBE_REFERENCED(x: SEXP) -> c_int {
    crate::sexp::accessors::NAMED(x)
}

fn as_integer(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    {
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP {
            return INTEGER_ELT(x, 0);
        }
        if t == SEXPTYPE::REALSXP {
            let v = REAL_ELT(x, 0);
            if v.is_nan() || v < c_int::MIN as f64 || v > c_int::MAX as f64 {
                return NA_INTEGER;
            }
            return v as c_int;
        }
        if t == SEXPTYPE::LGLSXP {
            return INTEGER_ELT(x, 0);
        }
        NA_INTEGER
    }
}

fn as_logical(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    {
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP {
            return INTEGER_ELT(x, 0);
        }
        if t == SEXPTYPE::INTSXP {
            return INTEGER_ELT(x, 0);
        }
        NA_INTEGER
    }
}

/* Fourier Transform for Univariate Spatial and Time Series */

pub fn fft(z: SEXP, inverse: SEXP) -> SEXP {
    use crate::main::errors::{Rf_error, Rf_error_cstr};

    let mut z = z;

    {
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
                Rf_error_cstr(c"non-numeric argument");
            }
        }
        let _z_guard = protect(z);

        let inverse = as_logical(inverse) != NA_INTEGER && as_logical(inverse) != 0;

        if LENGTH(z) > 1 {
            let d = getAttrib(z, R_DimSymbol());
            let mut buf = complex_vec(z);
            if d.is_null() || d == R_NilValue() {
                /* temporal transform */
                if !transform_line(&mut buf, 0, LENGTH(z) as usize, 1, inverse) {
                    Rf_error_cstr(c"FFT workspace exceeds session allocation limit");
                }
            } else {
                /* spatial transform */
                let ndims = LENGTH(d);
                let total = LENGTH(z) as usize;
                let mut stride = 1usize;
                for i in 0..(ndims as usize) {
                    let len = INTEGER_ELT(d, i as i32) as usize;
                    if len > 1 {
                        let block_len = stride * len;
                        for block in 0..(total / block_len) {
                            for lane in 0..stride {
                                if !transform_line(
                                    &mut buf,
                                    block * block_len + lane,
                                    len,
                                    stride,
                                    inverse,
                                ) {
                                    Rf_error_cstr(
                                        c"FFT workspace exceeds session allocation limit",
                                    );
                                }
                            }
                        }
                    }
                    stride *= len;
                }
            }
            store_complex(z, &buf);
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
pub fn do_fft(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

/* Fourier Transform for Vector-Valued ("multivariate") Series */

pub fn mvfft(z: SEXP, inverse: SEXP) -> SEXP {
    use crate::main::errors::{Rf_error, Rf_error_cstr};

    let mut z = z;
    {
        let d = getAttrib(z, R_DimSymbol());
        if d.is_null() || d == R_NilValue() || LENGTH(d) != 2 {
            Rf_error_cstr(c"vector-valued (multivariate) series required");
        }
        let n = INTEGER_ELT(d, 0);
        let p = INTEGER_ELT(d, 1);

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
                Rf_error_cstr(c"non-numeric argument");
            }
        }
        let _z_guard = protect(z);

        let inverse = as_logical(inverse) != NA_INTEGER && as_logical(inverse) != 0;

        if n > 1 {
            let mut buf = complex_vec(z);
            for i in 0..(p as usize) {
                if !transform_line(&mut buf, i * n as usize, n as usize, 1, inverse) {
                    Rf_error_cstr(c"FFT workspace exceeds session allocation limit");
                }
            }
            store_complex(z, &buf);
        }
    }
    z
}

/// Evaluator adapter for `mvfft(x, inverse = FALSE)`.
pub fn do_mvfft(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
        crate::main::errors::Rf_warning_cstr(
            c"nextn() found no solution < INT_MAX (the maximal integer)",
        );

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
        crate::main::errors::Rf_warning_cstr(c"nextn<64>() found no solution < UINT64_MAX");

        return 0;
    }
    n
}

pub fn nextn(mut n: SEXP, f: SEXP) -> SEXP {
    use crate::main::errors::{Rf_error, Rf_error_cstr};

    {
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
            Rf_error_cstr(c"no factors");
        }
        if nf < 0 {
            Rf_error_cstr(c"too many factors");
        }
        let factors_vec = crate::library::stats::checked::int_vec(f);
        let factors = factors_vec.as_slice();
        for &factor in factors {
            if factor == NA_INTEGER || factor <= 1 {
                Rf_error_cstr(c"invalid factors");
            }
        }

        let mut use_int = TYPEOF(n) == SEXPTYPE::INTSXP;
        if !use_int && TYPEOF(n) != SEXPTYPE::REALSXP {
            Rf_error_cstr(c"'n' must have typeof(.) \"integer\" or \"double\"");
        }
        let nn = XLENGTH(n);
        let mut _n_guard = protect(n);
        if !use_int && nn > 0 {
            let n_values = crate::library::stats::checked::real_vec(n);
            let mut n_max: c_double = -1.0;
            for &value in &n_values {
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
            let n_values = crate::library::stats::checked::int_vec(n);
            let r: Vec<c_int> = n_values
                .iter()
                .map(|&value| {
                    if value == NA_INTEGER {
                        NA_INTEGER
                    } else if value <= 1 {
                        1
                    } else {
                        nextn0(value, factors)
                    }
                })
                .collect();
            crate::library::stats::checked::set_integers(ans, &r);
        } else {
            let n_values = crate::library::stats::checked::real_vec(n);
            let mut r = vec![0.0; n_values.len()];
            for (i, &value) in n_values.iter().enumerate() {
                if ISNAN(value) {
                    r[i] = NA_REAL;
                } else if value <= 1.0 {
                    r[i] = 1.0;
                } else {
                    let max_dbl_int: u64 = 9007199254740992; // 2^53
                    let n_n = nextn0_64(value as u64, factors);
                    if n_n > max_dbl_int {
                        crate::main::errors::Rf_warning_cstr(
                            c"nextn() may not be exactly representable in R (as \"double\")",
                        );
                    }
                    r[i] = n_n as c_double;
                }
            }
            crate::library::stats::checked::set_reals(ans, &r);
        }

        ans
    }
}

/// GNU `nextn(n)` factors 2,3,5.
pub fn do_nextn(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let n = CAR(args);
    let f = if CDR(args).is_null() || CDR(args) == crate::sexp::globals::R_NilValue() {
        let fac = Rf_allocVector(SEXPTYPE::INTSXP.as_c_int(), 3);
        let _f = protect(fac);
        SET_INTEGER_ELT(fac, 0, 2);
        SET_INTEGER_ELT(fac, 1, 3);
        SET_INTEGER_ELT(fac, 2, 5);
        fac
    } else {
        CAR(CDR(args))
    };
    nextn(n, f)
}

/// GNU circular `convolve(x, y)` via fft.
pub fn do_convolve(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let mut x = coerceVector(CAR(args), SEXPTYPE::REALSXP.as_c_int());
    let _x = protect(x);
    let mut y = coerceVector(CAR(CDR(args)), SEXPTYPE::REALSXP.as_c_int());
    let _y = protect(y);
    let mut is_open = false;
    let mut is_filter = false;
    let mut p = CDR(CDR(args));
    while !p.is_null() && p != R_NilValue() {
        let s = CAR(p);
        if TYPEOF(s) == SEXPTYPE::STRSXP && XLENGTH(s) >= 1 {
            let t = crate::library::stats::checked::char_bytes(STRING_ELT(s, 0));
            match t.as_slice() {
                b"open" => is_open = true,
                b"filter" => is_filter = true,
                _ => {}
            }
        }
        p = CDR(p);
    }
    let n1_keep = XLENGTH(y) - 1;
    if is_open || is_filter {
        let nx = XLENGTH(x);
        let ny = XLENGTH(y);
        let n1 = ny - 1;
        let x2 = Rf_allocVector(SEXPTYPE::REALSXP.as_c_int(), (nx + n1) as c_int);
        let _x2 = protect(x2);
        for i in 0..n1 as usize {
            SET_REAL_ELT(x2, i as i32, 0.0);
        }
        for i in 0..nx as usize {
            SET_REAL_ELT(x2, (n1 as usize + i) as i32, REAL_ELT(x, i as i32));
        }
        let y2 = Rf_allocVector(SEXPTYPE::REALSXP.as_c_int(), (ny + nx - 1) as c_int);
        let _y2 = protect(y2);
        for i in 0..ny as usize {
            SET_REAL_ELT(y2, i as i32, REAL_ELT(y, i as i32));
        }
        for i in ny as usize..(ny + nx - 1) as usize {
            SET_REAL_ELT(y2, i as i32, 0.0);
        }
        x = x2;
        y = y2;
    }
    let n = XLENGTH(x);
    if n != XLENGTH(y) {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "length mismatch in convolution",
        );
    }
    let inv_false = Rf_ScalarLogical(0);
    let _f = protect(inv_false);
    let inv_true = Rf_ScalarLogical(1);
    let _t = protect(inv_true);
    let fx = fft(x, inv_false);
    let _fx = protect(fx);
    let fy = fft(y, inv_false);
    let _fy = protect(fy);
    let prod = Rf_allocVector(SEXPTYPE::CPLXSXP.as_c_int(), n as c_int);
    let _p = protect(prod);
    for i in 0..n as c_int {
        let a = COMPLEX_ELT(fx, i);
        let b = COMPLEX_ELT(fy, i);
        // a * conj(b)
        SET_COMPLEX_ELT(
            prod,
            i,
            Rcomplex {
                r: a.r * b.r + a.i * b.i,
                i: a.i * b.r - a.r * b.i,
            },
        );
    }
    let inv = fft(prod, inv_true);
    let _i = protect(inv);
    let ans = Rf_allocVector(SEXPTYPE::REALSXP.as_c_int(), n as c_int);
    let _a = protect(ans);
    let nf = n as f64;
    for i in 0..n as usize {
        SET_REAL_ELT(ans, i as i32, COMPLEX_ELT(inv, i as c_int).r / nf);
    }
    if is_filter {
        let n1 = n1_keep as usize;
        let out_n = n as usize - 2 * n1;
        let trimmed = Rf_allocVector(SEXPTYPE::REALSXP.as_c_int(), out_n as c_int);
        let _tr = protect(trimmed);
        for i in 0..out_n {
            SET_REAL_ELT(trimmed, i as i32, REAL_ELT(ans, (n1 + i) as i32));
        }
        trimmed
    } else {
        ans
    }
}

/// GNU `spec.pgram` default: detrend, taper 0.1, pad to `nextn`, scale by `u2`.
pub fn do_spec_pgram(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    use crate::sexp::accessors::{CAR, COMPLEX, INTEGER, REAL, SET_VECTOR_ELT, TYPEOF, XLENGTH};
    use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector3, Rf_mkString};
    use crate::sexp::protect::protect;
    let x = CAR(args);
    let n0 = XLENGTH(x) as usize;
    if n0 == 0 {
        return R_NilValue();
    }
    let mut v = vec![0.0; n0];
    for i in 0..n0 {
        v[i] = if TYPEOF(x) == SEXPTYPE::REALSXP {
            REAL_ELT(x, i as i32)
        } else if TYPEOF(x) == SEXPTYPE::INTSXP {
            INTEGER_ELT(x, i as i32) as f64
        } else {
            0.0
        };
    }
    let mut taper = 0.1;
    let mut detrend = true;
    let mut cell = CDR(args);
    while !cell.is_null() && cell != R_NilValue() {
        let tag = TAG(cell);
        let name = if !tag.is_null() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
            crate::library::stats::checked::char_string(PRINTNAME(tag))
        } else {
            String::new()
        };
        let val = CAR(cell);
        if name == "taper" {
            taper = if TYPEOF(val) == SEXPTYPE::REALSXP {
                REAL_ELT(val, 0)
            } else if TYPEOF(val) == SEXPTYPE::INTSXP {
                INTEGER_ELT(val, 0) as f64
            } else {
                taper
            };
        } else if name == "detrend" {
            detrend = as_logical(val) != 0;
        }
        cell = CDR(cell);
    }
    if taper < 0.0 {
        taper = 0.0;
    }
    if taper > 0.5 {
        taper = 0.5;
    }
    if detrend {
        let mean = v.iter().sum::<f64>() / n0 as f64;
        let nf = n0 as f64;
        let t0 = (nf + 1.0) / 2.0;
        let sumt2 = nf * (nf * nf - 1.0) / 12.0;
        let mut sumxt = 0.0;
        for i in 0..n0 {
            let t = (i as f64 + 1.0) - t0;
            sumxt += v[i] * t;
        }
        for i in 0..n0 {
            let t = (i as f64 + 1.0) - t0;
            v[i] = v[i] - mean - if sumt2 > 0.0 { sumxt * t / sumt2 } else { 0.0 };
        }
    }
    let m = ((n0 as f64) * taper).floor() as usize;
    for k in 0..m {
        let odd = (2 * k + 1) as f64;
        let w = 0.5 * (1.0 - (std::f64::consts::PI * odd / (2.0 * m as f64)).cos());
        v[k] *= w;
        v[n0 - 1 - k] *= w;
    }
    let n_pad = nextn0(n0 as c_int, &[2, 3, 5]) as usize;
    let xd = Rf_allocVector3(SEXPTYPE::REALSXP, n_pad as i64);
    let _xd = protect(xd);
    for i in 0..n0 {
        SET_REAL_ELT(xd, i as i32, v[i]);
    }
    for i in n0..n_pad {
        SET_REAL_ELT(xd, i as i32, 0.0);
    }
    let inv = Rf_ScalarLogical(0);
    let _inv = protect(inv);
    let z = fft(xd, inv);
    let _z = protect(z);
    let nspec = n_pad / 2;
    let u2 = 1.0 - (5.0 / 8.0) * taper * 2.0;
    let spec = Rf_allocVector3(SEXPTYPE::REALSXP, nspec as i64);
    let _s = protect(spec);
    let freq = Rf_allocVector3(SEXPTYPE::REALSXP, nspec as i64);
    let _f = protect(freq);
    let n0f = n0 as f64;
    for k in 1..=nspec {
        let c = COMPLEX_ELT(z, k as c_int);
        let p = (c.r * c.r + c.i * c.i) / n0f / u2;
        SET_REAL_ELT(spec, (k - 1) as i32, p);
        SET_REAL_ELT(freq, (k - 1) as i32, k as f64 / n_pad as f64);
    }
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    let _r = protect(result);
    SET_VECTOR_ELT(result, 0, spec);
    SET_VECTOR_ELT(result, 1, freq);
    crate::mainutils::essentials::set_string_names(
        result,
        &["spec".to_string(), "freq".to_string()],
    );
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_ClassSymbol(),
        crate::library::stats::checked::mk_string("spec"),
    );
    result
}

/// GNU `spectrum(x, method=)`.
pub fn do_spectrum(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let mut method_ar = false;
    let mut p = CDR(args);
    while !p.is_null() && p != R_NilValue() {
        let s = CAR(p);
        if TYPEOF(s) == SEXPTYPE::STRSXP && XLENGTH(s) >= 1 {
            let t = crate::library::stats::checked::char_bytes(STRING_ELT(s, 0));
            if t.as_slice() == b"ar" {
                method_ar = true;
            }
        }
        p = CDR(p);
    }
    if method_ar {
        crate::library::stats::filter::do_spec_ar(call, op, args, rho)
    } else {
        do_spec_pgram(call, op, args, rho)
    }
}
