//! Port of r-source/src/library/stats/src/ks.c — native Kolmogorov/Smirnov
//! routines registered by `useDynLib(stats)`: `C_pkolmogorov_two_limit`,
//! `C_pkolmogorov_two_exact` and `C_Smirnov_sim`.

use std::os::raw::{c_double, c_int};

use crate::sexp::accessors::{INTEGER, LENGTH, REAL};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// K2l — two-sample two-sided asymptotic distribution
// ---------------------------------------------------------------------------

/// Compute `\sum_{k=-inf}^{inf} (-1)^k e^{-2 k^2 x^2}` to tolerance `tol`.
fn k2l(x: c_double, lower: bool, tol: c_double) -> c_double {
    if x <= 0.0 {
        return if lower { 0.0 } else { 1.0 };
    }
    if x < 1.0 {
        let k_max = (2.0 - tol.ln()).sqrt() as i32;
        let w = x.ln();
        let z = -(std::f64::consts::FRAC_PI_2 * std::f64::consts::FRAC_PI_4) / (x * x);
        let mut s = 0.0;
        let mut k = 1;
        while k < k_max {
            s += ((k * k) as c_double * z - w).exp();
            k += 2;
        }
        let mut p = s * (2.0 * std::f64::consts::PI).sqrt(); // s / M_1_SQRT_2PI
        if !lower {
            p = 1.0 - p;
        }
        p
    } else {
        let z = -2.0 * x * x;
        let mut s = -1.0;
        let mut k;
        let mut new;
        if lower {
            k = 1;
            new = 1.0;
        } else {
            k = 2;
            new = 2.0 * z.exp();
        }
        let mut old = 0.0;
        while (old - new).abs() > tol {
            old = new;
            new += 2.0 * s * (z * (k * k) as c_double).exp();
            s *= -1.0;
            k += 1;
        }
        new
    }
}

/// `C_pkolmogorov_two_limit(q, lower.tail, tol)` — vectorised in `q`.
pub unsafe fn pkolmogorov_two_limit(sq: SEXP, slower: SEXP, stol: SEXP) -> SEXP {
    unsafe {
        let lower = crate::mainutils::coerce::asInteger(slower) != 0;
        let tol = crate::mainutils::coerce::asReal(stol);
        let ans = Rf_allocVector(SEXPTYPE::REALSXP, LENGTH(sq));
        let _guard = protect(ans);
        for i in 0..LENGTH(sq) as usize {
            *REAL(ans).add(i) = k2l(*REAL(sq).add(i), lower, tol);
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// K2x — one-sample two-sided exact distribution (Marsaglia/Tsang/Wang)
// ---------------------------------------------------------------------------

fn m_multiply(a: &[c_double], b: &[c_double], c: &mut [c_double], m: usize) {
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

fn m_power(a: &[c_double], e_a: i32, v: &mut Vec<c_double>, e_v: &mut i32, m: usize, n: i32) {
    if n == 1 {
        v.copy_from_slice(a);
        *e_v = e_a;
        return;
    }
    m_power(a, e_a, v, e_v, m, n / 2);
    let mut b = vec![0.0; m * m];
    m_multiply(v, v, &mut b, m);
    let e_b = 2 * (*e_v);
    if n % 2 == 0 {
        v.copy_from_slice(&b);
        *e_v = e_b;
    } else {
        m_multiply(a, &b, v, m);
        *e_v = e_a + e_b;
    }
    if v[(m / 2) * m + (m / 2)] > 1e140 {
        for value in v.iter_mut() {
            *value *= 1e-140;
        }
        *e_v += 140;
    }
}

fn pow_di(base: c_double, e: i32) -> c_double {
    let mut r = 1.0;
    for _ in 0..e {
        r *= base;
    }
    r
}

fn k2x(n: i32, d: c_double) -> c_double {
    let k = (n as c_double * d) as i32 + 1;
    let m = (2 * k - 1) as usize;
    let h = k as c_double - n as c_double * d;
    let mut mat = vec![0.0; m * m];
    for i in 0..m {
        for j in 0..m {
            if (i as i64 - j as i64 + 1) >= 0 {
                mat[i * m + j] = 1.0;
            }
        }
    }
    for i in 0..m {
        mat[i * m] -= pow_di(h, (i + 1) as i32);
        mat[(m - 1) * m + i] -= pow_di(h, (m - i) as i32);
    }
    if 2.0 * h - 1.0 > 0.0 {
        mat[(m - 1) * m] += pow_di(2.0 * h - 1.0, m as i32);
    }
    for i in 0..m {
        for j in 0..m {
            if (i as i64 - j as i64 + 1) > 0 {
                for g in 1..=(i - j + 1) {
                    mat[i * m + j] /= g as c_double;
                }
            }
        }
    }
    let e_h = 0;
    let mut q = vec![0.0; m * m];
    let mut e_q = 0;
    m_power(&mat, e_h, &mut q, &mut e_q, m, n);
    let mut s = q[((k - 1) as usize) * m + (k - 1) as usize];
    for i in 1..=n {
        s = s * i as c_double / n as c_double;
        if s < 1e-140 {
            s *= 1e140;
            e_q -= 140;
        }
    }
    s * pow_di(10.0, e_q)
}

/// `C_pkolmogorov_two_exact(q, n)` — vectorised in `q`.
pub unsafe fn pkolmogorov_two_exact(sq: SEXP, sn: SEXP) -> SEXP {
    unsafe {
        let n = crate::mainutils::coerce::asInteger(sn);
        let ans = Rf_allocVector(SEXPTYPE::REALSXP, LENGTH(sq));
        let _guard = protect(ans);
        for i in 0..LENGTH(sq) as usize {
            *REAL(ans).add(i) = k2x(n, *REAL(sq).add(i));
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// Smirnov_sim — random two-sample Smirnov statistics
// ---------------------------------------------------------------------------

/// `C_Smirnov_sim(r, sizes, B, two.sided)` — `B` simulated statistics from
/// random contingency tables with the given row/col totals (AS 159).
pub unsafe fn Smirnov_sim(sr: SEXP, sc: SEXP, sb: SEXP, twosided: SEXP) -> SEXP {
    unsafe {
        let sr =
            crate::main::coerce::coerceVector(sr, crate::sexp::ffi::SEXPTYPE::INTSXP.as_c_int());
        let _sr = protect(sr);
        let sc =
            crate::main::coerce::coerceVector(sc, crate::sexp::ffi::SEXPTYPE::INTSXP.as_c_int());
        let _sc = protect(sc);
        let nr = LENGTH(sr);
        let nc = LENGTH(sc);
        let b = crate::mainutils::coerce::asInteger(sb);
        if nc != 2 {
            crate::main::errors::Rf_error(
                b"Smirnov statistic only defined for two groups\0".as_ptr()
                    as *const std::os::raw::c_char,
            );
        }
        let isr = INTEGER(sr);
        let mut n = 0 as c_int;
        for i in 0..nr as usize {
            match n.checked_add(*isr.add(i)) {
                Some(total) => n = total,
                None => {
                    crate::main::errors::Rf_error(
                        b"Sample size too large\0".as_ptr() as *const std::os::raw::c_char
                    );
                    unreachable!("Rf_error raises an R error");
                }
            }
        }
        let mut observed = vec![0 as c_int; (nr * nc) as usize];
        let mut fact = vec![0.0 as c_double; (n + 1) as usize];
        let mut jwork = vec![0 as c_int; nc as usize];
        let ans = Rf_allocVector(SEXPTYPE::REALSXP, b);
        let _ans = protect(ans);
        // Smirnov_sim_wrk inlined: log-factorials, then B rcont2 draws.
        fact[0] = 0.0;
        if n >= 1 {
            fact[1] = 0.0;
        }
        for i in 2..=(n as usize) {
            fact[i] = fact[i - 1] + (i as c_double).ln();
        }
        crate::main::random::GetRNGstate();
        for iter in 0..b as usize {
            crate::library::stats::rcont::rcont2(
                nr,
                nc,
                isr,
                INTEGER(sc),
                n,
                fact.as_ptr(),
                jwork.as_mut_ptr(),
                observed.as_mut_ptr(),
            );
            let mut s = 0.0;
            let mut cs0 = 0;
            let mut cs1 = 0;
            for j in 0..nr as usize {
                cs0 += observed[j];
                cs1 += observed[nr as usize + j];
                let mut diff = cs0 as c_double / *INTEGER(sc).add(0) as c_double
                    - cs1 as c_double / *INTEGER(sc).add(1) as c_double;
                if *INTEGER(twosided).add(0) != 0 {
                    diff = diff.abs();
                }
                if diff > s {
                    s = diff;
                }
            }
            *REAL(ans).add(iter) = s;
        }
        crate::main::random::PutRNGstate();
        ans
    }
}
