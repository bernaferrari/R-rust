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

pub unsafe extern "C" fn c_eureka(
    lr: *mut std::ffi::c_void,
    r: *mut std::ffi::c_void,
    g: *mut std::ffi::c_void,
    f: *mut std::ffi::c_void,
    var: *mut std::ffi::c_void,
    a: *mut std::ffi::c_void,
) {
    unsafe {
        let lr = *(lr as *mut i32) as usize;
        if lr == 0 {
            return;
        }
        let r = r as *mut f64;
        let g = g as *mut f64;
        let f = f as *mut f64;
        let varp = var as *mut f64;
        let a = a as *mut f64;
        let at = |row: usize, col: usize| (col - 1) * lr + (row - 1);
        let mut v = *r;
        let mut d = *r.add(1);
        *a = 1.0;
        *f = *g.add(1) / v;
        let mut q = *f * *r.add(1);
        *varp = (1.0 - *f * *f) * *r;
        if lr == 1 {
            return;
        }
        for l in 2..=lr {
            *a.add(l - 1) = -d / v;
            if l > 2 {
                let l1 = (l - 2) / 2;
                let l2 = l1 + 1;
                for j in 2..=l2 {
                    let hold = *a.add(j - 1);
                    let k = l - j + 1;
                    *a.add(j - 1) = *a.add(j - 1) + *a.add(l - 1) * *a.add(k - 1);
                    *a.add(k - 1) = *a.add(k - 1) + *a.add(l - 1) * hold;
                }
                if 2 * l1 != l - 2 {
                    *a.add(l2) *= 1.0 + *a.add(l - 1);
                }
            }
            v += *a.add(l - 1) * d;
            *f.add(at(l, l)) = (*g.add(l) - q) / v;
            for j in 1..l {
                *f.add(at(l, j)) =
                    *f.add(at(l - 1, j)) + *f.add(at(l, l)) * *a.add(l - j);
            }
            *varp.add(l - 1) =
                *varp.add(l - 2) * (1.0 - *f.add(at(l, l)) * *f.add(at(l, l)));
            if l == lr {
                return;
            }
            d = 0.0;
            q = 0.0;
            for i in 1..=l {
                let k = l - i + 2;
                d += *a.add(i - 1) * *r.add(k - 1);
                q += *f.add(at(l, i)) * *r.add(k - 1);
            }
        }
    }
}
