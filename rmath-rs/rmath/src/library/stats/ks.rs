#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

//! Port of R's src/library/stats/src/ks.c
//!
//! Kolmogorov-Smirnov tests:
//! - Two-sample two-sided asymptotic distribution (K2l)
//! - Two-sample exact distributions (psmirnov_exact_*)
//! - One-sample two-sided exact distribution (K2x)
//! - Smirnov distribution simulation

use std::os::raw::{c_double, c_int};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_REAL, R_FINITE, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// Helper: R functions -- delegate to real implementations
// ---------------------------------------------------------------------------

unsafe fn asInteger(x: SEXP) -> c_int {
    crate::main::coerce::asInteger(x)
}

unsafe fn asReal(x: SEXP) -> c_double {
    crate::main::coerce::asReal(x)
}

unsafe fn coerceVector(x: SEXP, sexptype: SEXPTYPE) -> SEXP {
    crate::main::coerce::coerceVector(x, sexptype.0)
}

unsafe fn error(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    crate::main::errors::Rf_error(c_msg.as_ptr());
}

// ---------------------------------------------------------------------------
// Math constants
// ---------------------------------------------------------------------------

const M_PI_2: f64 = std::f64::consts::PI / 2.0;
const M_PI_4: f64 = std::f64::consts::PI / 4.0;
const M_1_SQRT_2PI: f64 = 1.0 / (2.0 * std::f64::consts::PI).sqrt();

// ---------------------------------------------------------------------------
// Two-sample two-sided asymptotic distribution
// ---------------------------------------------------------------------------

/// Compute the Kolmogorov-Smirnov two-sided asymptotic distribution.
///
/// K2l computes:
///   sum_{k=-inf}^{inf} (-1)^k e^{-2 k^2 x^2}
/// Uses the standard series for x >= 1, and the alternative series for x < 1.
fn K2l(x: f64, lower: bool, tol: f64) -> f64 {
    if x <= 0.0 {
        return if lower { 0.0 } else { 1.0 };
    } else if x < 1.0 {
        let k_max = (2.0 - tol.ln()).sqrt() as c_int;
        let w = x.ln();
        let z = -(M_PI_2 * M_PI_4) / (x * x);
        let mut s: f64 = 0.0;
        let mut k: c_int = 1;
        while k < k_max {
            s += (k as f64 * k as f64 * z - w).exp();
            k += 2;
        }
        let mut p = s / M_1_SQRT_2PI;
        if !lower {
            p = 1.0 - p;
        }
        p
    } else {
        let z = -2.0 * x * x;
        let mut s: f64 = -1.0;
        let mut k: c_int;
        let old: f64;
        let mut new_val: f64;

        if lower {
            k = 1;
            let mut old_v = 0.0;
            let mut new_v = 1.0;
            while (old_v - new_v).abs() > tol {
                old_v = new_v;
                new_v += 2.0 * s * (z * k as f64 * k as f64).exp();
                s *= -1.0;
                k += 1;
            }
            new_v
        } else {
            k = 2;
            let mut old_v = 0.0;
            let mut new_v = 2.0 * z.exp();
            while (old_v - new_v).abs() > tol {
                old_v = new_v;
                new_v += 2.0 * s * (z * k as f64 * k as f64).exp();
                s *= -1.0;
                k += 1;
            }
            new_v
        }
    }
}

/// R entry point: pkolmogorov_two_limit(sq, slower, stol)
pub unsafe fn pkolmogorov_two_limit(sq: SEXP, slower: SEXP, stol: SEXP) -> SEXP {
    let lower = asInteger(slower);
    let tol = asReal(stol);
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, LENGTH(sq)));
    for i in 0..LENGTH(sq) as usize {
        *REAL(ans).add(i) = K2l(*REAL(sq).add(i), lower != 0, tol);
    }
    Rf_unprotect(1);
    ans
}

// ---------------------------------------------------------------------------
// Two-sample exact distributions
// ---------------------------------------------------------------------------

type TestFn = unsafe fn(f64, f64, f64) -> c_int;

unsafe fn psmirnov_exact_test_one(q: f64, r: f64, s: f64) -> c_int {
    if (r - s) >= q { 1 } else { 0 }
}

unsafe fn psmirnov_exact_test_two(q: f64, r: f64, s: f64) -> c_int {
    if (r - s).abs() >= q { 1 } else { 0 }
}

unsafe fn psmirnov_exact_uniq_lower(q: f64, m: c_int, n: c_int, two: bool) -> f64 {
    let md = m as f64;
    let nd = n as f64;
    let test: TestFn = if two {
        psmirnov_exact_test_two
    } else {
        psmirnov_exact_test_one
    };

    let mut u = vec![0.0f64; (n + 1) as usize];
    u[0] = 1.0;
    for j in 1..=n as usize {
        if test(q, 0.0, j as f64 / nd) != 0 {
            u[j] = 0.0;
        } else {
            u[j] = u[j - 1];
        }
    }
    for i in 1..=m as usize {
        let w = i as f64 / (i + n as usize) as f64;
        if test(q, i as f64 / md, 0.0) != 0 {
            u[0] = 0.0;
        } else {
            u[0] = w * u[0];
        }
        for j in 1..=n as usize {
            if test(q, i as f64 / md, j as f64 / nd) != 0 {
                u[j] = 0.0;
            } else {
                u[j] = w * u[j] + u[j - 1];
            }
        }
    }
    u[n as usize]
}

unsafe fn psmirnov_exact_uniq_upper(q: f64, m: c_int, n: c_int, two: bool) -> f64 {
    let md = m as f64;
    let nd = n as f64;
    let test: TestFn = if two {
        psmirnov_exact_test_two
    } else {
        psmirnov_exact_test_one
    };

    let mut u = vec![0.0f64; (n + 1) as usize];
    u[0] = 0.0;
    for j in 1..=n as usize {
        if test(q, 0.0, j as f64 / nd) != 0 {
            u[j] = 1.0;
        } else {
            u[j] = u[j - 1];
        }
    }
    for i in 1..=m as usize {
        if test(q, i as f64 / md, 0.0) != 0 {
            u[0] = 1.0;
        }
        for j in 1..=n as usize {
            if test(q, i as f64 / md, j as f64 / nd) != 0 {
                u[j] = 1.0;
            } else {
                let v = i as f64 / (i + j) as f64;
                let w = j as f64 / (i + j) as f64;
                u[j] = v * u[j] + w * u[j - 1];
            }
        }
    }
    u[n as usize]
}

unsafe fn psmirnov_exact_ties_lower(q: f64, m: c_int, n: c_int, z: *const c_int, two: bool) -> f64 {
    let md = m as f64;
    let nd = n as f64;
    let test: TestFn = if two {
        psmirnov_exact_test_two
    } else {
        psmirnov_exact_test_one
    };

    let mut u = vec![0.0f64; (n + 1) as usize];
    u[0] = 1.0;
    for j in 1..=n as usize {
        if test(q, 0.0, j as f64 / nd) != 0 && *z.add(j) != 0 {
            u[j] = 0.0;
        } else {
            u[j] = u[j - 1];
        }
    }
    for i in 1..=m as usize {
        let w = i as f64 / (i + n as usize) as f64;
        if test(q, i as f64 / md, 0.0) != 0 && *z.add(i) != 0 {
            u[0] = 0.0;
        } else {
            u[0] = w * u[0];
        }
        for j in 1..=n as usize {
            if test(q, i as f64 / md, j as f64 / nd) != 0 && *z.add(i + j) != 0 {
                u[j] = 0.0;
            } else {
                u[j] = w * u[j] + u[j - 1];
            }
        }
    }
    u[n as usize]
}

unsafe fn psmirnov_exact_ties_upper(q: f64, m: c_int, n: c_int, z: *const c_int, two: bool) -> f64 {
    let md = m as f64;
    let nd = n as f64;
    let test: TestFn = if two {
        psmirnov_exact_test_two
    } else {
        psmirnov_exact_test_one
    };

    let mut u = vec![0.0f64; (n + 1) as usize];
    u[0] = 0.0;
    for j in 1..=n as usize {
        if test(q, 0.0, j as f64 / nd) != 0 && *z.add(j) != 0 {
            u[j] = 1.0;
        } else {
            u[j] = u[j - 1];
        }
    }
    for i in 1..=m as usize {
        if test(q, i as f64 / md, 0.0) != 0 && *z.add(i) != 0 {
            u[0] = 1.0;
        }
        for j in 1..=n as usize {
            if test(q, i as f64 / md, j as f64 / nd) != 0 && *z.add(i + j) != 0 {
                u[j] = 1.0;
            } else {
                let v = i as f64 / (i + j) as f64;
                let w = j as f64 / (i + j) as f64;
                u[j] = v * u[j] + w * u[j - 1];
            }
        }
    }
    u[n as usize]
}

/// R entry point: psmirnov_exact(sq, sm, sn, sz, stwo, slower)
pub unsafe fn psmirnov_exact(
    sq: SEXP,
    sm: SEXP,
    sn: SEXP,
    sz: SEXP,
    stwo: SEXP,
    slower: SEXP,
) -> SEXP {
    let m = asInteger(sm);
    let n = asInteger(sn);
    let two = asInteger(stwo);
    let lower = asInteger(slower);
    let ties = !sz.is_null();
    let z: *const c_int = if ties { INTEGER(sz) } else { ptr::null_mut() };

    let md = m as f64;
    let nd = n as f64;

    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, LENGTH(sq)));
    let p = REAL(ans);
    for i in 0..LENGTH(sq) as usize {
        let mut q = *REAL(sq).add(i);
        // Adjust q to avoid rounding error turning equality into inequality
        q = (0.5 + (q * md * nd - 1e-7).floor()) / (md * nd);
        if ties {
            if lower != 0 {
                *p.add(i) = psmirnov_exact_ties_lower(q, m, n, z, two != 0);
            } else {
                *p.add(i) = psmirnov_exact_ties_upper(q, m, n, z, two != 0);
            }
        } else {
            if lower != 0 {
                *p.add(i) = psmirnov_exact_uniq_lower(q, m, n, two != 0);
            } else {
                *p.add(i) = psmirnov_exact_uniq_upper(q, m, n, two != 0);
            }
        }
    }
    Rf_unprotect(1);
    ans
}

// ---------------------------------------------------------------------------
// One-sample two-sided exact distribution (Kolmogorov's distribution)
// ---------------------------------------------------------------------------

/// Compute x^n for integer n.
fn R_pow_di(x: f64, n: i32) -> f64 {
    if n == 0 {
        return 1.0;
    }
    if n < 0 {
        return R_pow_di(1.0 / x, -n);
    }
    let mut result = 1.0;
    let mut base = x;
    let mut exp = n;
    while exp > 0 {
        if exp % 2 == 1 {
            result *= base;
        }
        base *= base;
        exp /= 2;
    }
    result
}

/// Matrix multiplication for K2x.
fn m_multiply(a: &[f64], b: &[f64], c: &mut [f64], m: usize) {
    for i in 0..m {
        for j in 0..m {
            let mut s = 0.0;
            for k in 0..m {
                s += a[i * m + k] * b[k * m + j];
            }
            c[i * m + j] = s;
        }
    }
}

/// Matrix power for K2x (recursive, using exponent scaling to avoid overflow).
fn m_power(a: &[f64], e_a: i32, v: &mut [f64], e_v: &mut i32, m: usize, n: i32) {
    if n == 1 {
        v.copy_from_slice(a);
        *e_v = e_a;
        return;
    }
    m_power(a, e_a, v, e_v, m, n / 2);
    let mut b = vec![0.0f64; m * m];
    m_multiply(v, v, &mut b, m);
    let mut e_b = 2 * (*e_v);
    if n % 2 == 0 {
        v.copy_from_slice(&b);
        *e_v = e_b;
    } else {
        let mut tmp = vec![0.0f64; m * m];
        m_multiply(a, &b, &mut tmp, m);
        v.copy_from_slice(&tmp);
        *e_v = e_a + e_b;
    }
    // Scale to avoid overflow
    let mid = m / 2;
    if v[mid * m + mid] > 1e140 {
        for elem in v.iter_mut() {
            *elem *= 1e-140;
        }
        *e_v += 140;
    }
}

/// Compute Kolmogorov's distribution for one-sample two-sided test.
///
/// Based on Marsaglia, Tsang & Wang (2003), JSS 8(18).
fn K2x(n: c_int, d: f64) -> f64 {
    let k = (n as f64 * d) as c_int + 1;
    let m = 2 * k - 1;
    let h = k as f64 - n as f64 * d;
    let mm = m as usize;

    let mut h_mat = vec![0.0f64; mm * mm];
    let mut q_mat = vec![0.0f64; mm * mm];

    // Initialize H
    for i in 0..mm {
        for j in 0..mm {
            if (i as c_int - j as c_int + 1) < 0 {
                h_mat[i * mm + j] = 0.0;
            } else {
                h_mat[i * mm + j] = 1.0;
            }
        }
    }
    for i in 0..mm {
        h_mat[i * mm] -= R_pow_di(h, (i + 1) as i32);
        h_mat[(mm - 1) * mm + i] -= R_pow_di(h, (mm - i) as i32);
    }
    if 2.0 * h - 1.0 > 0.0 {
        h_mat[(mm - 1) * mm] += R_pow_di(2.0 * h - 1.0, m as i32);
    }
    for i in 0..mm {
        for j in 0..mm {
            let diff = i as c_int - j as c_int + 1;
            if diff > 0 {
                for g in 1..=diff {
                    h_mat[i * mm + j] /= g as f64;
                }
            }
        }
    }

    let e_h: i32 = 0;
    let mut e_q: i32 = 0;
    m_power(&h_mat, e_h, &mut q_mat, &mut e_q, mm, n);

    let mut s = q_mat[(k as usize - 1) * mm + (k as usize - 1)];
    for i in 1..=n as usize {
        s = s * i as f64 / n as f64;
        if s < 1e-140 {
            s *= 1e140;
            e_q -= 140;
        }
    }
    s *= R_pow_di(10.0, e_q);
    s
}

/// R entry point: pkolmogorov_two_exact(sq, sn)
pub unsafe fn pkolmogorov_two_exact(sq: SEXP, sn: SEXP) -> SEXP {
    let n = asInteger(sn);
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, LENGTH(sq)));
    for i in 0..LENGTH(sq) as usize {
        *REAL(ans).add(i) = K2x(n, *REAL(sq).add(i));
    }
    Rf_unprotect(1);
    ans
}

// ---------------------------------------------------------------------------
// Smirnov simulation
// ---------------------------------------------------------------------------

/// Smirnov distribution simulation worker.
fn Smirnov_sim_wrk(
    nrow: c_int,
    ncol: c_int,
    nrowt: *const c_int,
    ncolt: *const c_int,
    n: c_int,
    b: c_int,
    observed: &mut [c_int],
    twosided: c_int,
    fact: &mut [f64],
    jwork: &mut [c_int],
    results: &mut [f64],
) {
    // Calculate log-factorials
    fact[0] = 0.0;
    if n >= 1 {
        fact[1] = 0.0;
    }
    for i in 2..=n as usize {
        fact[i] = fact[i - 1] + (i as f64).ln();
    }

    // Note: GetRNGstate()/PutRNGstate() are no-ops in this port
    // since we don't have full RNG state management yet.

    for iter in 0..b as usize {
        // Call rcont2 to generate a random contingency table
        unsafe {
            crate::library::stats::rcont::rcont2(
                nrow,
                ncol,
                nrowt,
                ncolt,
                n,
                fact.as_ptr(),
                jwork.as_mut_ptr(),
                observed.as_mut_ptr(),
            );
        }

        let mut s: f64 = 0.0;
        let mut diff: f64 = 0.0;
        let mut cs0: c_int = 0;
        let mut cs1: c_int = 0;
        for j in 0..nrow as usize {
            cs0 += observed[j];
            cs1 += observed[nrow as usize + j];
            diff =
                cs0 as f64 / ncolt.add(0).read() as f64 - cs1 as f64 / ncolt.add(1).read() as f64;
            if twosided != 0 {
                diff = diff.abs();
            }
            if diff > s {
                s = diff;
            }
        }
        results[iter] = s;
    }
}

/// R entry point: Smirnov_sim(sr, sc, sB, twosided)
pub unsafe fn Smirnov_sim(sr: SEXP, sc: SEXP, sB: SEXP, twosided: SEXP) -> SEXP {
    let sr = Rf_protect(coerceVector(sr, SEXPTYPE::INTSXP.as_c_int()));
    let sc = Rf_protect(coerceVector(sc, SEXPTYPE::INTSXP.as_c_int()));
    let nr = LENGTH(sr);
    let nc = LENGTH(sc);
    let b = asInteger(sB);
    if nc != 2 {
        error("Smirnov statistic only defined for two groups");
    }
    let mut n: c_int = 0;
    let isr = INTEGER(sr);
    for i in 0..nr as usize {
        if n > c_int::MAX - *isr.add(i) {
            error("Sample size too large");
        }
        n += *isr.add(i);
    }

    let mut observed = vec![0i32; (nr * nc) as usize];
    let mut fact = vec![0.0f64; (n + 1) as usize];
    let mut jwork = vec![0i32; nc as usize];

    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, b));
    let results = std::slice::from_raw_parts_mut(REAL(ans), b as usize);

    Smirnov_sim_wrk(
        nr,
        nc,
        isr,
        INTEGER(sc),
        n,
        b,
        &mut observed,
        *INTEGER(twosided),
        &mut fact,
        &mut jwork,
        results,
    );

    Rf_unprotect(3);
    ans
}
