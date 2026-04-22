// Ported from R's nmath/polygamma.c
//
// Compute the derivatives of the psi function and polygamma functions.
//
// The following definitions are used in dpsifn:
//
// Definition 1
//   psi(x) = d/dx (ln(gamma(x))), the first derivative of the log gamma function.
//
// Definition 2
//   psi(k,x) = d^k/dx^k (psi(x)), the k-th derivative of psi(x).
//
// dpsifn computes a sequence of scaled derivatives of the psi function;
// i.e. for fixed x and m it computes the m-member sequence
//
//   (-1)^(k+1) / gamma(k+1) * psi(k,x)  for k = n,...,n+m-1
//
// Original by Amos, D.E. (Fortran), Ross Ihaka (C Translation),
// Martin Maechler (x < 0, and psigamma()).

use crate::nmath::constants::*;
use crate::nmath::special::mlutils::R_pow_di;
use crate::nmath::utils::*;
use libm::{cos, exp, fabs, log, pow, round, sin};

const N_MAX: i32 = 100;

// Bernoulli Numbers
const BVALUES: [f64; 23] = [
    1.00000000000000000e+00,
    -5.00000000000000000e-01,
    1.66666666666666667e-01,
    -3.33333333333333333e-02,
    2.38095238095238095e-02,
    -3.33333333333333333e-02,
    7.57575757575757576e-02,
    -2.53113553113553114e-01,
    1.16666666666666667e+00,
    -7.09215686274509804e+00,
    5.49711779448621554e+01,
    -5.29124242424242424e+02,
    6.19212318840579710e+03,
    -8.65802531135531136e+04,
    1.42551716666666667e+06,
    -2.72982310678160920e+07,
    6.01580873900642368e+08,
    -1.51163157670921569e+10,
    4.29614643061166667e+11,
    -1.37116552050883328e+13,
    4.88332318973593167e+14,
    -1.92965793419400681e+16,
    0.0, // placeholder (bvalues[22] unused in original)
];

/// Compute d_n(x) = (d/dx)^n cot(x); cot(x) := cos(x) / sin(x)
fn d_n_cot(x: f64, n: i32) -> f64 {
    match n {
        0 => cos(x) / sin(x),
        1 => -1.0 / R_pow_di(sin(x), 2),
        2 => 2.0 * cos(x) / R_pow_di(sin(x), 3),
        3 => {
            let sin2 = R_pow_di(sin(x), 2);
            -2.0 * (3.0 - 2.0 * sin2) / R_pow_di(sin2, 2)
        }
        4 => {
            let co = cos(x);
            8.0 * co * (R_pow_di(co, 2) + 2.0) / R_pow_di(sin(x), 5)
        }
        5 => {
            let co2 = R_pow_di(cos(x), 2);
            -8.0 * (2.0 * R_pow_di(co2, 2) + 11.0 * co2 + 2.0) / R_pow_di(sin(x), 6)
        }
        _ => ML_NAN,
    }
}

/// Core implementation of dpsifn.
///
/// Computes a sequence of scaled derivatives of the psi function.
///
/// Returns (ans, nz, ierr) where:
/// - ans: vector of length m with the computed values
/// - nz: underflow flag (number of trailing zeros)
/// - ierr: error flag (0=ok, 1=input error, 2=overflow, 3=dimension error, 4=not implemented)
fn dpsifn(x: f64, n: i32, kode: i32, m: i32) -> (Vec<f64>, i32, i32) {
    let mut ans = vec![0.0; m as usize];
    let mut nz: i32 = 0;

    if n < 0 || kode < 1 || kode > 2 || m < 1 {
        return (ans, nz, 1);
    }

    if x <= 0.0 {
        // Reflection Formula: Abramowitz & Stegun 6.4.7
        if x == round(x) {
            // non-positive integer: +Inf or NaN depends on n
            for j in 0..m {
                let k = (j + n) as i32;
                ans[j as usize] = if (k % 2) != 0 { ML_POSINF } else { ML_NAN };
            }
            return (ans, nz, 0);
        }
        let (ans_ref, nz_ref, ierr_ref) = dpsifn(1.0 - x, n, 1, m);
        if ierr_ref != 0 {
            return (ans, nz, ierr_ref);
        }
        ans = ans_ref;
        nz = nz_ref;

        // For now: only work for n in {0,1,..,5}
        if n > 5 {
            return (ans, nz, 4);
        }

        let xpi = x * std::f64::consts::PI;

        let mut t1 = 1.0_f64;
        let mut t2 = 1.0_f64;
        let mut s = 1.0_f64;
        let mut k: i32 = 0;
        let mut j: i32 = k - n;
        while j < m {
            t1 *= std::f64::consts::PI;
            if k >= 2 {
                t2 *= k as f64;
            }
            if j >= 0 {
                ans[j as usize] = s * (ans[j as usize] + t1 / t2 * d_n_cot(xpi, k));
            }
            k += 1;
            j += 1;
            s = -s;
        }
        return (ans, nz, 0);
    }

    // else: x > 0
    let xln = log(x);
    if kode == 1 && m == 1 {
        let lrg = 1.0 / (2.0 * f64::EPSILON);
        if n == 0 && x * xln > lrg {
            ans[0] = -xln;
            return (ans, nz, 0);
        } else if n >= 1 && x > n as f64 * lrg {
            ans[0] = exp(-(n as f64) * xln) / n as f64;
            return (ans, nz, 0);
        }
    }

    let nx_val = imin2(-1021, 1024);
    let r1m5: f64 = std::f64::consts::LOG10_2;
    let r1m4: f64 = f64::EPSILON * 0.5;
    let wdtol = fmax2(r1m4, 0.5e-18);

    let elim = 2.302 * (nx_val as f64 * r1m5 - 3.0);
    let rln = fmin2(r1m5 * 53.0, 18.06);
    let mut fln = fmax2(rln, 3.0) - 3.0;
    let yint = 3.50 + 0.40 * fln;
    let slope = 0.21 + fln * (0.0006038 * fln + 0.008677);

    let mut mm = m;
    let mut trm = [0.0_f64; 23];
    let mut trmr = [0.0_f64; N_MAX as usize + 1];

    loop {
        let nn = n + mm - 1;
        let fn_ = nn as f64;
        let t = (fn_ + 1.0) * xln;

        if fabs(t) > elim {
            if t <= 0.0 {
                return (ans, nz, 2);
            }
        } else {
            if x < wdtol {
                ans[0] = R_pow_di(x, -n - 1);
                if mm != 1 {
                    for k_ in 1..mm {
                        ans[k_ as usize] = ans[(k_ - 1) as usize] / x;
                    }
                }
                if n == 0 && kode == 2 {
                    ans[0] += xln;
                }
                return (ans, nz, 0);
            }

            let xm = yint + slope * fn_;
            let mx = xm as i32 + 1;
            let xmin = mx as f64;
            if n != 0 {
                let xm2 = -2.302 * rln - fmin2(0.0, xln);
                let arg = fmin2(0.0, xm2 / n as f64);
                let eps = exp(arg);
                let xm3 = if fabs(arg) < 1.0e-3 { -arg } else { 1.0 - eps };
                fln = x * xm3 / eps;
                let xm4 = xmin - x;
                if xm4 > 7.0 && fln < 15.0 {
                    break;
                }
            }
            let mut xdmy = x;
            let mut xdmln = xln;
            let mut xinc = 0.0_f64;
            if x < xmin {
                let nx2 = x as i32;
                xinc = xmin - nx2 as f64;
                xdmy = x + xinc;
                xdmln = log(xdmy);
            }

            // generate w(n+mm-1, x) by the asymptotic expansion
            let t_val = fn_ * xdmln;
            let t1_val = xdmln + xdmln;
            let t2_val = t_val + xdmln;
            let tk = fmax2(fabs(t_val), fmax2(fabs(t1_val), fabs(t2_val)));
            if tk <= elim {
                // L10 path: asymptotic expansion
                let tss = exp(-t_val);
                let tt = 0.5 / xdmy;
                let mut t1_l = tt;
                let tst = wdtol * tt;
                if nn != 0 {
                    t1_l = tt + 1.0 / fn_;
                }
                let rxsq = 1.0 / (xdmy * xdmy);
                let ta = 0.5 * rxsq;
                let mut t_l = (fn_ + 1.0) * ta;
                let mut s_l = t_l * BVALUES[2];
                if fabs(s_l) >= tst {
                    let mut tk_l = 2.0_f64;
                    let mut k_idx: usize = 4;
                    while k_idx <= 22 {
                        t_l = t_l
                            * ((tk_l + fn_ + 1.0) / (tk_l + 1.0))
                            * ((tk_l + fn_) / (tk_l + 2.0))
                            * rxsq;
                        trm[k_idx] = t_l * BVALUES[k_idx - 1];
                        if fabs(trm[k_idx]) < tst {
                            break;
                        }
                        s_l += trm[k_idx];
                        tk_l += 2.0;
                        k_idx += 1;
                    }
                }
                let mut s_l = (s_l + t1_l) * tss;
                let nx_l = if xinc != 0.0 { xinc as i32 } else { 0 };

                if xinc != 0.0 {
                    let np = nn + 1;
                    if nx_l > N_MAX {
                        return (ans, nz, 3);
                    }
                    if nn == 0 {
                        // L20 then L30
                        for i in 1..=nx_l {
                            s_l += 1.0 / (x + (nx_l - i) as f64);
                        }
                        if kode != 2 {
                            ans[0] = s_l - xdmln;
                        } else if (xdmy - x).abs() > 0.0 {
                            let xq = xdmy / x;
                            ans[0] = s_l - log(xq);
                        }
                        return (ans, nz, 0);
                    }
                    let mut xm_l = xinc - 1.0;
                    let mut fx = x + xm_l;
                    for i in 1..=nx_l {
                        trmr[i as usize] = pow(fx, -np as f64);
                        s_l += trmr[i as usize];
                        xm_l -= 1.0;
                        fx = x + xm_l;
                    }
                }
                ans[(mm - 1) as usize] = s_l;
                if fn_ == 0.0 {
                    // L30
                    if kode != 2 {
                        ans[0] = s_l - xdmln;
                    } else if (xdmy - x).abs() > 0.0 {
                        let xq = xdmy / x;
                        ans[0] = s_l - log(xq);
                    }
                    return (ans, nz, 0);
                }

                // generate lower derivatives, j < n+mm-1
                let mut fn_mut = fn_;
                let mut tss_mut = tss;
                for j in 2..=mm {
                    fn_mut -= 1.0;
                    tss_mut *= xdmy;
                    let mut t1_j = tt;
                    if fn_mut != 0.0 {
                        t1_j = tt + 1.0 / fn_mut;
                    }
                    let t_j = (fn_mut + 1.0) * ta;
                    let mut s_j = t_j * BVALUES[2];
                    if fabs(s_j) >= tst {
                        let mut tk_j = 4.0 + fn_mut;
                        let mut k_j: usize = 4;
                        while k_j <= 22 {
                            trm[k_j] = trm[k_j] * (fn_mut + 1.0) / tk_j;
                            if fabs(trm[k_j]) < tst {
                                break;
                            }
                            s_j += trm[k_j];
                            tk_j += 2.0;
                            k_j += 1;
                        }
                    }
                    let mut s_j = (s_j + t1_j) * tss_mut;
                    if xinc != 0.0 {
                        if fn_mut == 0.0 {
                            // L20 then L30
                            for i in 1..=nx_l {
                                s_j += 1.0 / (x + (nx_l - i) as f64);
                            }
                            if kode != 2 {
                                ans[0] = s_j - xdmln;
                            } else if (xdmy - x).abs() > 0.0 {
                                let xq = xdmy / x;
                                ans[0] = s_j - log(xq);
                            }
                            return (ans, nz, 0);
                        }
                        let mut xm_j = xinc - 1.0;
                        let mut fx_j = x + xm_j;
                        for i in 1..=nx_l {
                            trmr[i as usize] = trmr[i as usize] * fx_j;
                            s_j += trmr[i as usize];
                            xm_j -= 1.0;
                            fx_j = x + xm_j;
                        }
                    }
                    ans[(mm - j) as usize] = s_j;
                    if fn_mut == 0.0 {
                        // L30
                        if kode != 2 {
                            ans[0] = s_j - xdmln;
                        } else if (xdmy - x).abs() > 0.0 {
                            let xq = xdmy / x;
                            ans[0] = s_j - log(xq);
                        }
                        return (ans, nz, 0);
                    }
                }
                return (ans, nz, 0);
            }
        }
        nz += 1;
        mm -= 1;
        ans[mm as usize] = 0.0;
        if mm == 0 {
            return (ans, nz, 0);
        }
    }

    // Series computation for large n
    let nn = fln as i32 + 1;
    let np = n + 1;
    let t1 = (n + 1) as f64 * xln;
    let mut t = exp(-t1);
    let mut s = t;
    let mut den = x;
    for i in 1..=nn {
        den += 1.0;
        trm[i as usize] = pow(den, -np as f64);
        s += trm[i as usize];
    }
    ans[0] = s;
    if n == 0 && kode == 2 {
        ans[0] = s + xln;
    }

    if mm != 1 {
        let tol = wdtol / 5.0;
        let mut j: i32 = 1;
        while j < mm {
            t /= x;
            s = t;
            let tols = t * tol;
            den = x;
            let mut i: i32 = 1;
            while i <= nn {
                den += 1.0;
                trm[i as usize] /= den;
                s += trm[i as usize];
                if trm[i as usize] < tols {
                    break;
                }
                i += 1;
            }
            ans[j as usize] = s;
            j += 1;
        }
    }

    (ans, nz, 0)
}

mod imp {
    use super::*;

    pub fn psigamma(x: f64, deriv: f64) -> f64 {
        if isnan(x) {
            return x;
        }
        let deriv = r_forceint(deriv);
        let n = deriv as i32;
        if n > N_MAX {
            return ML_NAN;
        }
        let (ans_vec, _nz, ierr) = dpsifn(x, n, 1, 1);
        if ierr != 0 {
            return ML_NAN;
        }
        let mut ans = -ans_vec[0];
        for k in 1..=n {
            ans *= -(k as f64);
        }
        ans
    }

    pub fn digamma(x: f64) -> f64 {
        if isnan(x) {
            return x;
        }
        let (ans_vec, _nz, ierr) = dpsifn(x, 0, 1, 1);
        if ierr != 0 {
            return ML_NAN;
        }
        -ans_vec[0]
    }

    pub fn trigamma(x: f64) -> f64 {
        if isnan(x) {
            return x;
        }
        let (ans_vec, _nz, ierr) = dpsifn(x, 1, 1, 1);
        if ierr != 0 {
            return ML_NAN;
        }
        ans_vec[0]
    }

    pub fn tetragamma(x: f64) -> f64 {
        if isnan(x) {
            return x;
        }
        let (ans_vec, _nz, ierr) = dpsifn(x, 2, 1, 1);
        if ierr != 0 {
            return ML_NAN;
        }
        -2.0 * ans_vec[0]
    }

    pub fn pentagamma(x: f64) -> f64 {
        if isnan(x) {
            return x;
        }
        let (ans_vec, _nz, ierr) = dpsifn(x, 3, 1, 1);
        if ierr != 0 {
            return ML_NAN;
        }
        6.0 * ans_vec[0]
    }
}

// =====================================================================
// Public API (Rust)
// =====================================================================

/// n-th derivative of psi(x); e.g., psigamma(x, 0) == digamma(x).
pub fn psigamma(x: f64, deriv: f64) -> f64 {
    imp::psigamma(x, deriv)
}

/// The digamma function: psi(x) = d/dx ln(gamma(x)).
/// Uses the asymptotic expansion for large x, and the recurrence
/// relation psi(x) = psi(x+1) - 1/x for small x.
pub fn digamma(x: f64) -> f64 {
    if isnan(x) {
        return x;
    }
    if x <= 0.0 && x == libm::floor(x) {
        return ML_POSINF; // poles at non-positive integers
    }

    let mut result = 0.0;
    let mut x = x;

    // Use recurrence to shift x to a large value
    while x < 6.0 {
        result -= 1.0 / x;
        x += 1.0;
    }

    // Asymptotic expansion: psi(x) ~ ln(x) - 1/(2x) - sum B_{2k}/(2k * x^{2k})
    result += log(x) - 0.5 / x;
    let x2 = x * x;
    let s = 1.0 / x2;
    result -= s
        * (1.0 / 12.0
            - s * (1.0 / 120.0 - s * (1.0 / 252.0 - s * (1.0 / 240.0 - s * (1.0 / 132.0)))));
    result
}

/// The trigamma function: psi'(x) = d^2/dx^2 ln(gamma(x)).
pub fn trigamma(x: f64) -> f64 {
    imp::trigamma(x)
}

/// The tetragamma function: psi''(x) = d^3/dx^3 ln(gamma(x)).
pub fn tetragamma(x: f64) -> f64 {
    imp::tetragamma(x)
}

/// The pentagamma function: psi'''(x) = d^4/dx^4 ln(gamma(x)).
pub fn pentagamma(x: f64) -> f64 {
    imp::pentagamma(x)
}

// =====================================================================
// C FFI shims
// =====================================================================

pub fn Rf_digamma(x: f64) -> f64 {
    imp::digamma(x)
}

pub fn digamma_c(x: f64) -> f64 {
    imp::digamma(x)
}

pub fn Rf_trigamma(x: f64) -> f64 {
    imp::trigamma(x)
}

pub fn trigamma_c(x: f64) -> f64 {
    imp::trigamma(x)
}

pub fn Rf_tetragamma(x: f64) -> f64 {
    imp::tetragamma(x)
}

pub fn tetragamma_c(x: f64) -> f64 {
    imp::tetragamma(x)
}

pub fn Rf_pentagamma(x: f64) -> f64 {
    imp::pentagamma(x)
}

pub fn pentagamma_c(x: f64) -> f64 {
    imp::pentagamma(x)
}

pub fn Rf_psigamma(x: f64, deriv: f64) -> f64 {
    imp::psigamma(x, deriv)
}

pub fn psigamma_c(x: f64, deriv: f64) -> f64 {
    imp::psigamma(x, deriv)
}
