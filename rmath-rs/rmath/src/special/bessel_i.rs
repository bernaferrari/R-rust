#![allow(
    unused_assignments,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::mut_range_bound
)]
// Ported from R's nmath/bessel_i.c
//
// From http://www.netlib.org/specfun/ribesl Fortran translated by f2c,
//   Martin Maechler, ETH Zurich
//
// Computes modified Bessel functions of the first kind,
// I_nu(x), for non-negative argument x and non-negative order nu,
// with or without exponential scaling.

use crate::constants::*;
use crate::error::*;
use crate::special::bessel_k::{bessel_k, bessel_k_ex};
use crate::special::cospi::sinpi;
use crate::special::gamma::gammafn;
use crate::special::mlutils::R_pow_di;
use crate::utils::*;
use libm::*;

// =====================================================================
// Constants from bessel.h
// =====================================================================

const NSIG_BESS: f64 = 16.0;
const ENSIG_BESS: f64 = 1e16;
const RTNSIG_BESS: f64 = 1e-4;
const ENMTEN_BESS: f64 = 8.9e-308;
const ENTEN_BESS: f64 = 1e308;
const EXPARG_BESS: f64 = 709.0;
const XLRG_BESS_IJ: f64 = 1e5;

/// Minimum of two ints
#[inline(always)]
fn min0(x: i32, y: i32) -> i32 {
    if x <= y { x } else { y }
}

// =====================================================================
// Internal I_bessel
// =====================================================================

/// Calculates modified Bessel functions I_{n+alpha} (x) for non-negative
/// argument x, and non-negative order n+alpha.
///
/// # Parameters
/// - `x`: Non-negative argument
/// - `alpha`: Fractional part of order, 0 <= ALPHA < 1
/// - `nb`: Number of functions to calculate, nb >= 1
/// - `ize`: 1 = unscaled, 2 = exponentially scaled (I*EXP(-X))
/// - `bi`: Output vector (0-indexed in Rust), length nb
///
/// # Returns
/// - `ncalc`: Number of orders successfully calculated
fn i_bessel(x: f64, alpha: f64, nb: i32, ize: i32, bi: &mut [f64]) -> i32 {
    let const_ = 1.585;

    let nu: f64 = alpha;
    let twonu: f64 = ldexp(nu, 1); // = 2 * nu

    // Check for X, NB, or IZE out of range
    if nb > 0 && x >= 0.0 && 0.0 <= nu && nu < 1.0 && 1 <= ize && ize <= 2 {
        let mut ncalc = nb;

        if ize == 1 && x > EXPARG_BESS {
            // x > 709
            for k in 0..(nb as usize) {
                bi[k] = ML_POSINF;
            }
            return ncalc;
        }
        if ize == 2 && x > XLRG_BESS_IJ {
            for k in 0..(nb as usize) {
                bi[k] = 0.0;
            }
            return ncalc;
        }

        let intx = x as i32;

        if x >= RTNSIG_BESS {
            // "non-small" x (>= 1e-4)
            // Initialize the forward sweep, the P-sequence of Olver
            let nbmx = nb - intx;
            let mut n: i32 = intx + 1;
            let mut en: f64 = ((n + n) as f64) + twonu;
            let mut plast: f64 = 1.0;
            let mut p: f64 = en / x;

            // Calculate general significance test
            let mut test: f64 = ENSIG_BESS + ENSIG_BESS;
            if (intx << 1) > (NSIG_BESS as i32) * 5 {
                test = sqrt(test * p);
            } else {
                test /= R_pow_di(const_, intx);
            }

            let mut nstart: i32 = 0;
            let mut psave: f64 = 0.0;
            let mut psavel: f64 = 0.0;
            let mut overflow_occurred = false;

            if nbmx >= 3 {
                // Calculate P-sequence until N = NB-1
                // Check for possible overflow
                let tover = ENTEN_BESS / ENSIG_BESS;
                nstart = intx + 2;
                let nend = nb - 1;
                for k in nstart..=nend {
                    n = k;
                    en += 2.0;
                    let pold = plast;
                    plast = p;
                    p = en * plast / x + pold;
                    if p > tover {
                        // To avoid overflow, divide P-sequence by TOVER.
                        // Calculate P-sequence until ABS(P) > 1.
                        let tover2 = ENTEN_BESS;
                        p /= tover2;
                        plast /= tover2;
                        psave = p;
                        psavel = plast;
                        nstart = n + 1;
                        loop {
                            n += 1;
                            en += 2.0;
                            let pold2 = plast;
                            plast = p;
                            p = en * plast / x + pold2;
                            if p > 1.0 {
                                break;
                            }
                        }

                        let bb = en / x;
                        // Calculate backward test, and find NCALC,
                        // the highest N such that the test is passed.
                        test = pold * plast / ENSIG_BESS;
                        test *= 0.5 - 0.5 / (bb * bb);
                        p = plast * tover2;
                        n -= 1;
                        en -= 2.0;
                        let nend2 = min0(nb, n);
                        let mut reached_l90 = false;
                        for l in nstart..=nend2 {
                            ncalc = l;
                            let pold3 = psavel;
                            psavel = psave;
                            psave = en * psavel / x + pold3;
                            if psave * psavel > test {
                                reached_l90 = true;
                                break;
                            }
                        }
                        if !reached_l90 {
                            ncalc = nend2 + 1;
                        }
                        ncalc -= 1;
                        overflow_occurred = true;
                        break;
                    }
                }

                if !overflow_occurred {
                    n = nend;
                    en = ((n + n) as f64) + twonu;
                    // Calculate special significance test for NBMX > 2.
                    test = fmax2(test, sqrt(plast * ENSIG_BESS) * sqrt(p + p));
                }
            }

            if !overflow_occurred {
                // Calculate P-sequence until significance test passed.
                loop {
                    n += 1;
                    en += 2.0;
                    let pold4 = plast;
                    plast = p;
                    p = en * plast / x + pold4;
                    if !(p < test) {
                        break;
                    }
                }
            }

            // L120:
            // Initialize the backward recursion and the normalization sum.
            n += 1;
            en += 2.0;
            let mut bb: f64 = 0.0;
            let mut aa: f64 = 1.0 / p;
            let mut em: f64 = (n - 1) as f64;
            let mut empal: f64 = em + nu;
            let mut emp2al: f64 = em - 1.0 + twonu;
            let mut sum: f64 = aa * empal * emp2al / em;
            let mut nend: i32 = n - nb;

            if nend < 0 {
                // N < NB, so store BI[N] and set higher orders to 0.
                bi[(n - 1) as usize] = aa;
                nend = -nend;
                for l in 1..=nend {
                    bi[(n + l - 1) as usize] = 0.0;
                }
            } else {
                if nend > 0 {
                    // Recur backward via difference equation,
                    // calculating (but not storing) BI[N], until N = NB.
                    for _l in 1..=nend {
                        n -= 1;
                        en -= 2.0;
                        let cc = bb;
                        bb = aa;
                        // Re-normalize to avoid overflow
                        if nend > 100 && aa > 1e200 {
                            // multiply by 2^-900 = 1.18e-271
                            let _cc = ldexp(cc, -900);
                            bb = ldexp(bb, -900);
                            sum = ldexp(sum, -900);
                        }
                        aa = en * bb / x + cc;
                        em -= 1.0;
                        emp2al -= 1.0;
                        if n == 1 {
                            break;
                        }
                        if n == 2 {
                            emp2al = 1.0;
                        }
                        empal -= 1.0;
                        sum = (sum + aa * empal) * emp2al / em;
                    }
                }
                // Store BI[NB]
                bi[(n - 1) as usize] = aa;
                if nb <= 1 {
                    sum = sum + sum + aa;
                    // goto L230
                } else {
                    // Calculate and Store BI[NB-1]
                    n -= 1;
                    en -= 2.0;
                    bi[(n - 1) as usize] = en * aa / x + bb;
                    if n == 1 {
                        // goto L220
                    } else {
                        em -= 1.0;
                        if n == 2 {
                            emp2al = 1.0;
                        } else {
                            emp2al -= 1.0;
                        }
                        empal -= 1.0;
                        sum = (sum + bi[(n - 1) as usize] * empal) * emp2al / em;

                        nend = n - 2;
                        if nend > 0 {
                            // Calculate via difference equation
                            // and store BI[N], until N = 2.
                            for _l in 1..=nend {
                                n -= 1;
                                en -= 2.0;
                                bi[(n - 1) as usize] =
                                    en * bi[n as usize] / x + bi[(n + 1) as usize];
                                em -= 1.0;
                                if n == 2 {
                                    emp2al = 1.0;
                                } else {
                                    emp2al -= 1.0;
                                }
                                empal -= 1.0;
                                sum = (sum + bi[(n - 1) as usize] * empal) * emp2al / em;
                            }
                        }
                        // Calculate BI[1]
                        bi[0] = 2.0 * empal * bi[1] / x + bi[2];
                    }
                    // L220:
                    sum = sum + sum + bi[0];
                }

                // L230:
                // Normalize. Divide all BI[N] by sum.
                if nu != 0.0 {
                    sum *= gammafn(1.0 + nu) * pow(x * 0.5, -nu);
                }
                if ize == 1 {
                    sum *= exp(-x);
                }
                let mut aa2 = ENMTEN_BESS;
                if sum > 1.0 {
                    aa2 *= sum;
                }
                for i in 0..(nb as usize) {
                    if bi[i] < aa2 {
                        bi[i] = 0.0;
                    } else {
                        bi[i] /= sum;
                    }
                }
                return ncalc;
            }
        } else {
            // small x < 1e-4
            // Two-term ascending series for small X.
            let mut aa: f64 = 1.0;
            let mut empal: f64 = 1.0 + nu;
            let halfx: f64 = ldexp(x, -1); // = x / 2
            if nu != 0.0 {
                aa = pow(halfx, nu) / gammafn(empal);
            }
            if ize == 2 {
                aa *= exp(-x);
            }
            let bb = halfx * halfx;
            bi[0] = aa + aa * bb / empal;
            if x != 0.0 && bi[0] == 0.0 {
                ncalc = 0;
            }
            if nb > 1 {
                if x == 0.0 {
                    for i in 1..(nb as usize) {
                        bi[i] = 0.0;
                    }
                } else {
                    // Calculate higher-order functions.
                    let cc = halfx;
                    let mut tover = (ENMTEN_BESS + ENMTEN_BESS) / x;
                    if bb != 0.0 {
                        tover = ENMTEN_BESS / bb;
                    }
                    for i in 1..(nb as usize) {
                        aa /= empal;
                        empal += 1.0;
                        aa *= cc;
                        if aa <= tover * empal {
                            aa = 0.0;
                        }
                        bi[i] = aa + aa * bb / empal;
                        if bi[i] == 0.0 && ncalc > (i as i32) {
                            ncalc = (i - 1) as i32;
                        }
                    }
                }
            }
        }
        return ncalc;
    }

    // argument out of range
    bi[0] = 0.0;
    min0(nb, 0) - 1
}

// =====================================================================
// Public API
// =====================================================================

/// Modified Bessel function of the first kind, I_nu(x).
#[must_use]
///
/// Ported from R's bessel_i() in bessel_i.c.
///
/// # Arguments
/// * `x` - Non-negative argument
/// * `alpha` - Order (may be negative)
/// * `expo` - 1.0 for unscaled, 2.0 for exponentially scaled (I*EXP(-X))
///
/// # Returns
/// I_alpha(x) or exp(-x)*I_alpha(x) depending on expo
pub fn bessel_i(x: f64, alpha: f64, expo: f64) -> f64 {
    // NaNs propagated correctly
    if isnan(x) || isnan(alpha) {
        return x + alpha;
    }
    if x < 0.0 {
        ml_warning(ME_RANGE, "bessel_i");
        return ML_NAN;
    }

    let ize = expo as i32;
    let na = floor(alpha);
    if alpha < 0.0 {
        // Using Abramowitz & Stegun 9.6.2 & 9.6.6
        return bessel_i(x, -alpha, expo)
            + if alpha == na {
                0.0 // sin(pi * alpha) = 0
            } else {
                bessel_k(x, -alpha, expo) * if ize == 1 { 2.0 } else { 2.0 * exp(-2.0 * x) }
                    / std::f64::consts::PI
                    * sinpi(-alpha)
            };
    }

    let nb = 1 + (na as i32); // nb-1 <= alpha < nb
    let alpha_mod = alpha - ((nb - 1) as f64);

    let mut bi = vec![0.0; nb as usize];
    let ncalc = i_bessel(x, alpha_mod, nb, ize, &mut bi);
    if ncalc != nb {
        if ncalc < 0 {
            ml_warning(ME_RANGE, "bessel_i");
        } else {
            ml_warning(ME_PRECISION, "bessel_i");
        }
    }
    bi[(nb - 1) as usize]
}

/// C FFI wrapper for bessel_i
#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn bessel_i_c(x: f64, alpha: f64, expo: f64) -> f64 {
    bessel_i(x, alpha, expo)
}

// =====================================================================
// bessel_i_ex: version accepting a pre-allocated work array
// =====================================================================

/// Modified version of bessel_i(), accepting a work array instead of allocating one.
#[must_use]
pub fn bessel_i_ex(x: f64, alpha: f64, expo: f64, bi: &mut [f64]) -> f64 {
    // NaNs propagated correctly
    if isnan(x) || isnan(alpha) {
        return x + alpha;
    }
    if x < 0.0 {
        ml_warning(ME_RANGE, "bessel_i");
        return ML_NAN;
    }

    let ize = expo as i32;
    let na = floor(alpha);
    if alpha < 0.0 {
        // Using Abramowitz & Stegun 9.6.2 & 9.6.6
        return bessel_i_ex(x, -alpha, expo, bi)
            + if alpha == na {
                0.0
            } else {
                bessel_k_ex(x, -alpha, expo, bi) * if ize == 1 { 2.0 } else { 2.0 * exp(-2.0 * x) }
                    / std::f64::consts::PI
                    * sinpi(-alpha)
            };
    }

    let nb: i32 = 1 + (na as i32);
    let alpha_mod = alpha - ((nb - 1) as f64);

    let ncalc = i_bessel(x, alpha_mod, nb, ize, bi);
    if ncalc != nb {
        if ncalc < 0 {
            ml_warning(ME_RANGE, "bessel_i");
        } else {
            ml_warning(ME_PRECISION, "bessel_i");
        }
    }
    bi[(nb - 1) as usize]
}

/// C FFI wrapper for bessel_i_ex
#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn bessel_i_ex_c(x: f64, alpha: f64, expo: f64, bi: *mut f64, nb: i32) -> f64 {
    if bi.is_null() || nb <= 0 {
        return ML_NAN;
    }
    let bi_slice = unsafe { std::slice::from_raw_parts_mut(bi, nb as usize) };
    bessel_i_ex(x, alpha, expo, bi_slice)
}
