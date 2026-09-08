//! Burg's algorithm for AR model estimation
//! Port of r-source/src/library/stats/src/burg.c

use std::os::raw::{c_double, c_int};
use std::slice;

use crate::main::coerce::{asInteger, coerceVector};
use crate::mainutils::errors::Rf_error;
use crate::sexp::accessors::{LENGTH, REAL, SET_VECTOR_ELT};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::protect;

fn burg_values(
    x: &[c_double],
    pmax: usize,
    coefs: &mut [c_double],
    var1: &mut [c_double],
    var2: &mut [c_double],
) {
    let n = x.len();
    let mut u = vec![0.0; n];
    let mut v = vec![0.0; n];
    let mut u0 = vec![0.0; n];

    coefs.fill(0.0);

    let mut sum = 0.0;
    for (t, value) in x.iter().rev().enumerate() {
        u[t] = *value;
        v[t] = *value;
    }

    for value in x {
        sum += value * value;
    }
    var1[0] = sum / n as c_double;
    var2[0] = var1[0];

    for p in 1..=pmax {
        sum = 0.0;
        let mut d = 0.0;
        for t in p..n {
            sum += v[t] * u[t - 1];
            d += v[t] * v[t] + u[t - 1] * u[t - 1];
        }
        let phii = 2.0 * sum / d;
        coefs[pmax * (p - 1) + (p - 1)] = phii;
        if p > 1 {
            for j in 1..p {
                coefs[p - 1 + pmax * (j - 1)] =
                    coefs[p - 2 + pmax * (j - 1)] - phii * coefs[p - 2 + pmax * (p - j - 1)];
            }
        }
        u0.copy_from_slice(&u);
        for t in p..n {
            u[t] = u0[t - 1] - phii * v[t];
            v[t] -= phii * u0[t - 1];
        }
        var1[p] = var1[p - 1] * (1.0 - phii * phii);
        let mut d = 0.0;
        for t in p..n {
            d += v[t] * v[t] + u[t] * u[t];
        }
        var2[p] = d / (2.0 * (n - p) as c_double);
    }
}

fn error(msg: &'static [u8]) -> ! {
    unsafe {
        Rf_error(msg.as_ptr() as *const _);
    }
    unreachable!("Rf_error returned");
}

fn non_negative_usize(value: c_int, name: &'static [u8]) -> usize {
    if value < 0 {
        error(name);
    }
    value as usize
}

pub unsafe fn Burg(x: SEXP, order: SEXP) -> SEXP {
    let x = unsafe { coerceVector(x, SEXPTYPE::REALSXP.as_c_int()) };
    let _x_guard = protect(x);
    let n = unsafe { LENGTH(x) };
    let pmax = unsafe { asInteger(order) };
    let pmax_usize = non_negative_usize(pmax, b"'order.max' must be non-negative\0");
    if n <= pmax {
        error(b"'order.max' must be smaller than the number of observations\0");
    }

    let coefs = unsafe { Rf_allocVector(SEXPTYPE::REALSXP, pmax * pmax) };
    let _coefs_guard = protect(coefs);
    let var1 = unsafe { Rf_allocVector(SEXPTYPE::REALSXP, pmax + 1) };
    let _var1_guard = protect(var1);
    let var2 = unsafe { Rf_allocVector(SEXPTYPE::REALSXP, pmax + 1) };
    let _var2_guard = protect(var2);

    let x_values = unsafe { slice::from_raw_parts(REAL(x), n as usize) };
    let coefs_values = unsafe { slice::from_raw_parts_mut(REAL(coefs), pmax_usize * pmax_usize) };
    let var1_values = unsafe { slice::from_raw_parts_mut(REAL(var1), pmax_usize + 1) };
    let var2_values = unsafe { slice::from_raw_parts_mut(REAL(var2), pmax_usize + 1) };
    burg_values(x_values, pmax_usize, coefs_values, var1_values, var2_values);

    let ans = unsafe { Rf_allocVector(SEXPTYPE::VECSXP, 3) };
    let _ans_guard = protect(ans);
    unsafe {
        SET_VECTOR_ELT(ans, 0, coefs);
        SET_VECTOR_ELT(ans, 1, var1);
        SET_VECTOR_ELT(ans, 2, var2);
    }
    ans
}
