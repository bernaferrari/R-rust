#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

//! Burg's algorithm for AR model estimation
//! Port of r-source/src/library/stats/src/burg.c

use std::os::raw::{c_double, c_int};

use crate::main::coerce::{asInteger, coerceVector};
use crate::sexp::accessors::{LENGTH, REAL, SET_VECTOR_ELT, TYPEOF};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

unsafe fn burg(
    n: c_int,
    x: *const c_double,
    pmax: c_int,
    coefs: *mut c_double,
    var1: *mut c_double,
    var2: *mut c_double,
) {
    let u = vec![0.0f64; n as usize];
    let mut u = u;
    let v = vec![0.0f64; n as usize];
    let mut v = v;
    let u0 = vec![0.0f64; n as usize];
    let mut u0 = u0;

    // Zero out coefs
    for i in 0..(pmax * pmax) as usize {
        *coefs.add(i) = 0.0;
    }

    let mut sum = 0.0;
    let mut t: c_int = 0;
    while t < n {
        u[t as usize] = *x.add((n - 1 - t) as usize);
        v[t as usize] = *x.add((n - 1 - t) as usize);
        sum += *x.add(t as usize) * *x.add(t as usize);
        t += 1;
    }
    *var1 = sum / n as c_double;
    *var2 = *var1;

    let mut p: c_int = 1;
    while p <= pmax {
        sum = 0.0;
        let mut d: c_double = 0.0;
        let mut t = p;
        while t < n {
            sum += v[t as usize] * u[(t - 1) as usize];
            d += v[t as usize] * v[t as usize] + u[(t - 1) as usize] * u[(t - 1) as usize];
            t += 1;
        }
        let phii = 2.0 * sum / d;
        *coefs.add((pmax * (p - 1) + (p - 1)) as usize) = phii;
        if p > 1 {
            let mut j: c_int = 1;
            while j < p {
                *coefs.add((p - 1 + pmax * (j - 1)) as usize) = *coefs
                    .add((p - 2 + pmax * (j - 1)) as usize)
                    - phii * *coefs.add((p - 2 + pmax * (p - j - 1)) as usize);
                j += 1;
            }
        }
        // update u and v
        let mut t: c_int = 0;
        while t < n {
            u0[t as usize] = u[t as usize];
            t += 1;
        }
        let mut t = p;
        while t < n {
            u[t as usize] = u0[(t - 1) as usize] - phii * v[t as usize];
            v[t as usize] = v[t as usize] - phii * u0[(t - 1) as usize];
            t += 1;
        }
        *var1.add(p as usize) = *var1.add((p - 1) as usize) * (1.0 - phii * phii);
        let mut d: c_double = 0.0;
        let mut t = p;
        while t < n {
            d += v[t as usize] * v[t as usize] + u[t as usize] * u[t as usize];
            t += 1;
        }
        *var2.add(p as usize) = d / (2.0 * (n - p) as c_double);
        p += 1;
    }
}

pub unsafe fn Burg(x: SEXP, order: SEXP) -> SEXP {
    let x = Rf_protect(coerceVector(x, SEXPTYPE::REALSXP.0));
    let n = LENGTH(x);
    let pmax = asInteger(order);
    let coefs = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, pmax * pmax));
    let var1 = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, pmax + 1));
    let var2 = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, pmax + 1));
    burg(n, REAL(x), pmax, REAL(coefs), REAL(var1), REAL(var2));
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, 3));
    SET_VECTOR_ELT(ans, 0, coefs);
    SET_VECTOR_ELT(ans, 1, var1);
    SET_VECTOR_ELT(ans, 2, var2);
    Rf_unprotect(5);
    ans
}
