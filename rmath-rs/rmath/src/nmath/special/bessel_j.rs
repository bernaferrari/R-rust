// Ported from R's nmath/bessel_j.c
//
// Original by W. J. Cody, Applied Mathematics Division,
// Argonne National Laboratory, Argonne, IL 60439.
//
// From http://www.netlib.org/specfun/rjbesl
//   Fortran translated by f2c, Martin Maechler, ETH Zurich
// Additional code for nu == alpha < 0  MM

use crate::nmath::constants::*;
use crate::nmath::error::*;
use crate::nmath::special::bessel_y::{bessel_y, bessel_y_ex};
use crate::nmath::special::cospi::{cospi, sinpi};
use crate::nmath::special::gamma::gammafn;
use crate::nmath::utils::*;
use libm::*;

// =====================================================================
// Constants from bessel.h
// =====================================================================

const NSIG_BESS: f64 = 16.0;
const ENSIG_BESS: f64 = 1e16;
const RTNSIG_BESS: f64 = 1e-4;
const ENMTEN_BESS: f64 = 8.9e-308;
const ENTEN_BESS: f64 = 1e308;

const XLRG_BESS_IJ: f64 = 1e5;

/// 2^-800 = 1.4996968....e-241
const VERY_SMALL_NU: f64 = f64::from_bits(0x0010_0000_0000_0000); // 2^-800

/// Minimum of two ints (from bessel_j.c: #define min0(x, y) (((x) <= (y)) ? (x) : (y)))
#[inline(always)]
fn min0(x: i32, y: i32) -> i32 {
    if x <= y { x } else { y }
}

// =====================================================================
// Internal J_bessel
// =====================================================================

/// Calculates Bessel functions J_{n+alpha} (x) for non-negative argument x,
/// and non-negative order n+alpha, n = 0, 1, ..., nb-1.
///
/// # Parameters
/// - `x`: Non-negative argument
/// - `alpha`: Fractional part of order, 0 <= ALPHA < 1
/// - `nb`: Number of functions to calculate, nb >= 1
/// - `b`: Output vector (0-indexed in Rust, 1-indexed in C), length nb
///
/// # Returns
/// - `ncalc`: Number of orders successfully calculated
fn j_bessel(x: f64, alpha: f64, nb: i32, b: &mut [f64]) -> i32 {
    // Mathematical constants
    let pi2: f64 = 0.636619772367581343075535; // 2 / pi

    let twopi1: f64 = 6.28125; // first few significant digits of 2*pi
    let twopi2: f64 = 0.001935307179586476925286767; // 2*pi - twopi1

    // In C code, --b makes b 1-indexed. We keep 0-indexed in Rust.

    let mut nu: f64 = alpha; // in [0, 1)
    let mut twonu: f64 = ldexp(nu, 1); // = 2 * nu

    // Declare all variables that are used across branches
    let mut alpem: f64;
    let mut alp2em: f64;
    let mut aa: f64;
    let mut bb: f64;
    let mut cc: f64;
    let mut p: f64;
    let mut s: f64;
    let mut en: f64;
    let mut sum: f64;
    let mut tover: f64;

    // Check for out of range arguments.
    if nb > 0 && x >= 0.0 && 0.0 <= nu && nu < 1.0 {
        let mut ncalc = nb;
        // Initialize result array to zero.
        for i in 0..(nb as usize) {
            b[i] = 0.0;
        }

        if x > XLRG_BESS_IJ {
            ml_warning(ME_RANGE, "J_bessel");
            return ncalc;
        }

        let intx = x as i32;

        /*===================================================================
        Branch into  3 cases :
        1) use 2-term ascending series for small X
        2) use asymptotic form for large X when NB is not too large
        3) use recursion otherwise;
         3b:  if 0 < |nu| = |alpha| < very_small_nu, use nu = very_small_nu
        ===================================================================*/

        if x < RTNSIG_BESS {
            /* ============= branch 1)
            Two-term ascending series for small X. */

            alpem = 1.0 + nu;
            let halfx: f64 = if x > ENMTEN_BESS { 0.5 * x } else { 0.0 };
            aa = if nu != 0.0 {
                pow(halfx, nu) / (nu * gammafn(nu))
            } else {
                1.0
            };
            bb = if x + 1.0 > 1.0 { -halfx * halfx } else { 0.0 }; // manual underflow
            b[0] = aa + aa * bb / alpem;
            if x != 0.0 && b[0] == 0.0 {
                ncalc = 0;
            }

            if nb != 1 {
                if x <= 0.0 {
                    for n in 2..=(nb as usize) {
                        b[n - 1] = 0.0;
                    }
                } else {
                    /* Calculate higher order functions. */
                    if bb == 0.0 {
                        tover = (ENMTEN_BESS + ENMTEN_BESS) / x;
                    } else {
                        tover = ENMTEN_BESS / bb;
                    }
                    cc = halfx;
                    for n in 2..=(nb as usize) {
                        aa /= alpem;
                        alpem += 1.0;
                        aa *= cc;
                        if aa <= tover * alpem {
                            aa = 0.0;
                        }
                        b[n - 1] = aa + aa * bb / alpem;
                        if b[n - 1] == 0.0 && ncalc > n as i32 {
                            ncalc = (n - 1) as i32;
                        }
                    }
                }
            }
        } else if x > 25.0 && nb <= intx + 1 {
            /* ============= branch 2)
            Asymptotic series for X > 25 (and not much larger nb) */

            // m := #{terms in asymptotic series} to be used
            let m_asym: i32 = if x >= 130.0 {
                4
            } else if x >= 35.0 {
                8
            } else {
                11
            }; // ==> k := 2m <= 22

            /* Factorial(N) */
            let fact: [f64; 25] = [
                1.0,
                1.0,
                2.0,
                6.0,
                24.0,
                120.0,
                720.0,
                5040.0,
                40320.0,
                362880.0,
                3628800.0,
                39916800.0,
                479001600.0,
                6227020800.0,
                87178291200.0,
                1.307674368e12,
                2.0922789888e13,
                3.55687428096e14,
                6.402373705728e15,
                1.21645100408832e17,
                2.43290200817664e18,
                5.109094217170944e19,
                1.12400072777760768e21,
                2.585201673888497664e22,
                6.2044840173323943936e23,
            ];

            let xc = sqrt(pi2 / x);
            let xin = 1.0 / (64.0 * x * x);
            let xm = 4.0 * (m_asym as f64);

            /* Argument reduction for SIN and COS routines. */
            let mut t = trunc(x / (twopi1 + twopi2) + 0.5);
            let z = (x - t * twopi1) - t * twopi2 - (nu + 0.5) / pi2;
            let mut vsin = sin(z);
            let mut vcos = cos(z);
            let mut gnu = twonu;

            for i in 0..2 {
                s = (xm - 1.0 - gnu) * (xm - 1.0 + gnu) * xin * 0.5;
                t = (gnu - (xm - 3.0)) * (gnu + (xm - 3.0));
                let mut k: i32 = m_asym + m_asym;
                let mut t1 = (gnu - (xm + 1.0)) * (gnu + (xm + 1.0));
                let mut capp = s * t / fact[k as usize];
                let mut capq = s * t1 / fact[(k + 1) as usize];
                let mut xk = xm;
                while k >= 4 {
                    /* k + 2(j-2) == 2m, for j = 1,... */
                    xk -= 4.0;
                    s = (xk - 1.0 - gnu) * (xk - 1.0 + gnu);
                    t1 = t;
                    t = (gnu - (xk - 3.0)) * (gnu + (xk - 3.0));
                    capp = (capp + 1.0 / fact[(k - 2) as usize]) * s * t * xin;
                    capq = (capq + 1.0 / fact[(k - 1) as usize]) * s * t1 * xin;
                    k -= 2;
                }
                capp += 1.0;
                capq = (capq + 1.0) * (gnu * gnu - 1.0) * (0.125 / x);
                b[i] = xc * (capp * vcos - capq * vsin);
                if nb == 1 {
                    return ncalc; // result: b[0]
                }

                /* vsin <--> vcos */
                t = vsin;
                vsin = -vcos;
                vcos = t;
                gnu += 2.0;
            } // end for i = 0,1

            /* If NB > 2, compute J(X,ORDER+I) for I = 2,.., NB-1 */
            if nb > 2 {
                gnu = twonu + 2.0;
                let mut i = 3;
                while i <= nb {
                    b[(i - 1) as usize] = gnu * b[(i - 2) as usize] / x - b[(i - 3) as usize];
                    gnu += 2.0;
                    i += 1;
                }
            }
        } else {
            /* rtnsig_BESS <= x && ( x <= 25 || intx+1 < nb )
            ============= branch 3)
            Use recurrence to generate results.
            First initialize the calculation of P*S. */

            if nu != 0.0 && fabs(nu) < VERY_SMALL_NU {
                nu = if nu < 0.0 {
                    -VERY_SMALL_NU
                } else {
                    VERY_SMALL_NU
                };
                twonu = ldexp(nu, 1);
            }

            let nbmx = nb - intx; // = nb - floor(x)
            let mut n: i32 = intx + 1;
            en = ((n + n) as f64) + twonu;
            p = en / x;

            /* Calculate general significance test. */
            let mut plast = 1.0;
            let mut pold: f64;
            let mut test = ENSIG_BESS + ENSIG_BESS;

            if nbmx >= 3 {
                /* Calculate P*S until N = NB-1. Check for possible overflow. */
                tover = ENTEN_BESS / ENSIG_BESS;
                let mut nstart: i32 = intx + 2;
                let nend: i32 = nb - 1;
                en = ((nstart + nstart) as f64) - 2.0 + twonu;
                let mut k: i32 = nstart;
                let mut overflow_occurred = false;
                while k <= nend {
                    n = k;
                    en += 2.0;
                    pold = plast;
                    plast = p;
                    p = en * plast / x - pold;
                    if p > tover {
                        overflow_occurred = true;
                        break;
                    }
                    k += 1;
                }

                if overflow_occurred {
                    /* To avoid overflow, divide P*S by TOVER.
                    Calculate P*S until ABS(P) > 1. */
                    tover = ENTEN_BESS;
                    p /= tover;
                    plast /= tover;
                    let mut psave = p;
                    let mut psavel = plast;
                    nstart = n + 1;
                    loop {
                        n += 1;
                        en += 2.0;
                        pold = plast;
                        plast = p;
                        p = en * plast / x - pold;
                        if !(p <= 1.0) {
                            break;
                        }
                    }

                    bb = en / x;

                    /* Calculate backward test and find NCALC,
                    the highest N such that the test is passed. */
                    test = pold * plast * (0.5 - 0.5 / (bb * bb));
                    test /= ENSIG_BESS;
                    p = plast * tover;
                    n -= 1;
                    en -= 2.0;
                    let nend2 = min0(nb, n);
                    let mut reached_l190 = false;
                    let mut ii: i32 = nstart;
                    while ii <= nend2 {
                        pold = psavel;
                        psavel = psave;
                        psave = en * psavel / x - pold;
                        if psave * psavel > test {
                            ncalc = ii - 1;
                            reached_l190 = true;
                            break;
                        }
                        ii += 1;
                    }
                    if !reached_l190 {
                        ncalc = nend2;
                    }
                    // goto L190 (fall through)
                } else {
                    /* get here only if *never* (p > tover) above */
                    n = nend;
                    en = ((n + n) as f64) + twonu;
                    /* Calculate special significance test for NBMX > 2. */
                    test = fmax2(test, sqrt(plast * ENSIG_BESS) * sqrt(p + p));
                }
            } // end if nbmx >= 3

            /* Calculate P*S until significance test passes. */
            loop {
                n += 1;
                en += 2.0;
                pold = plast;
                plast = p;
                p = en * plast / x - pold;
                if !(p < test) {
                    break;
                }
            }

            // L190:
            /* Initialize the backward recursion and the normalization sum. */
            n += 1;
            en += 2.0;
            bb = 0.0;
            aa = 1.0 / p;
            let mut m: i32 = n / 2;
            let mut em = m as f64;
            m = (n << 1) - (m << 2); /* = 2n - 4(n/2) = 0 for even, 2 for odd n */
            if m == 0 {
                sum = 0.0;
            } else {
                alpem = em - 1.0 + nu;
                alp2em = em + em + nu;
                sum = aa * alpem * alp2em / em;
            }

            let nend: i32 = n - nb;

            /* Recur backward via difference equation, calculating
            (but not storing) b[N], until N = NB. */
            for _i in 0..nend {
                n -= 1;
                en -= 2.0;
                cc = bb;
                bb = aa;
                aa = en * bb / x - cc;
                if m != 0 {
                    m = 0;
                } else {
                    m = 2;
                }
                if m != 0 {
                    em -= 1.0;
                    alp2em = em + em + nu;
                    if n == 1 {
                        break;
                    }
                    alpem = em - 1.0 + nu;
                    if alpem == 0.0 {
                        alpem = 1.0;
                    }
                    sum = (sum + aa * alp2em) * alpem / em;
                }
            }

            /* Store b[NB]. */
            b[(n - 1) as usize] = aa;

            if nend >= 0 {
                if n <= 1 {
                    sum += b[0] * if nu == 0.0 { 1.0 } else { nu };
                    // goto L250
                } else {
                    /* nb >= 2: Calculate and store b[NB-1]. */
                    n -= 1; // => n = nb-1
                    en -= 2.0;
                    b[(n - 1) as usize] = en * aa / x - bb;
                    if n == 1 {
                        // goto L240
                    } else {
                        if m != 0 {
                            m = 0;
                        } else {
                            m = 2;
                        }
                        if m != 0 {
                            em -= 1.0;
                            alp2em = em + em + nu;
                            alpem = em - 1.0 + nu;
                            if alpem == 0.0 {
                                alpem = 1.0;
                            }
                            sum = (sum + b[(n - 1) as usize] * alp2em) * alpem / em;
                        }

                        /* Calculate via difference equation and store b[N],
                        until N = 2. */
                        let mut nn = n - 1;
                        while nn >= 2 {
                            en -= 2.0;
                            b[(nn - 1) as usize] =
                                en * b[nn as usize] / x - b[(nn + 1 - 1) as usize];
                            if m != 0 {
                                m = 0;
                            } else {
                                m = 2;
                            }
                            if m != 0 {
                                em -= 1.0;
                                alp2em = em + em + nu;
                                alpem = em - 1.0 + nu;
                                if alpem == 0.0 {
                                    alpem = 1.0;
                                }
                                sum = (sum + b[(nn - 1) as usize] * alp2em) * alpem / em;
                            }
                            nn -= 1;
                        }

                        /* Calculate b[1]. */
                        b[0] = 2.0 * (nu + 1.0) * b[1] / x - b[2];
                    }

                    // L240:
                    em -= 1.0;
                    alp2em = em + em + nu;
                    if alp2em == 0.0 {
                        alp2em = 1.0;
                    }
                    sum += b[0] * alp2em;
                }

                // L250:
                /* Normalize. Divide all b[N] by sum. */
                // NB. ensured above that |nu| >= VERY_SMALL_NU
                if nu != 0.0 {
                    sum *= gammafn(nu) * pow(0.5 * x, -nu);
                }

                for n in 1..=(nb as usize) {
                    b[n - 1] /= sum;
                }
            } else {
                /* nend < 0: backward recursion didn't reach NB.
                Still need to compute and store remaining b[] values
                and then normalize. */
                // Continue backward recursion, now storing values.
                loop {
                    n -= 1;
                    en -= 2.0;
                    cc = bb;
                    bb = aa;
                    aa = en * bb / x - cc;
                    if m != 0 {
                        m = 0;
                    } else {
                        m = 2;
                    }
                    if m != 0 {
                        em -= 1.0;
                        alp2em = em + em + nu;
                        alpem = em - 1.0 + nu;
                        if alpem == 0.0 {
                            alpem = 1.0;
                        }
                        sum = (sum + aa * alp2em) * alpem / em;
                    }
                    b[(n - 1) as usize] = aa;
                    if n <= 1 {
                        break;
                    }
                }

                if n == 1 {
                    sum += b[0] * if nu == 0.0 { 1.0 } else { nu };
                }

                /* L250: Normalize. */
                if nu != 0.0 {
                    sum *= gammafn(nu) * pow(0.5 * x, -nu);
                }

                for n in 1..=(nb as usize) {
                    b[n - 1] /= sum;
                }
            }
        }

        return ncalc;
    }

    /* Error return -- X, NB, or ALPHA = nu is out of range */
    b[0] = 0.0;
    min0(nb, 0) - 1 // <= -1
}

// =====================================================================
// Public API
// =====================================================================

/// Bessel function of the first kind, J_nu(x).
///
/// Ported from R's bessel_j() in bessel_j.c.
///
/// # Arguments
/// * `x` - Non-negative argument
/// * `alpha` - Order (may be negative)
///
/// # Returns
/// J_alpha(x)
pub fn bessel_j(x: f64, alpha: f64) -> f64 {
    // NaNs propagated correctly
    if isnan(x) || isnan(alpha) {
        return x + alpha;
    }
    if x < 0.0 {
        ml_warning(ME_RANGE, "bessel_j");
        return ML_NAN;
    }
    // ==> x >= 0 from now on
    let na = floor(alpha);
    if alpha < 0.0 {
        /* Using Abramowitz & Stegun  9.1.2
         * this may not be quite optimal (CPU and accuracy wise) */
        let part1: f64 = if alpha - na == 0.5 {
            0.0
        } else {
            bessel_j(x, -alpha) * cospi(alpha)
        };
        let part2: f64 = if alpha == na {
            0.0
        } else {
            /* bessel_y is in a separate module; call it via a free function
             * that will be available once bessel_y.rs is ported. */
            bessel_y(x, -alpha) * sinpi(alpha)
        };
        return part1 + part2;
    } else if alpha > 1e7 {
        ml_warning(ME_RANGE, "bessel_j");
        return ML_NAN;
    }
    let nb = 1 + (na as i32); /* nb-1 <= alpha < nb */
    let alpha_mod = alpha - ((nb - 1) as f64); // ==> alpha' in [0, 1)

    let mut b = vec![0.0; nb as usize];
    let ncalc = j_bessel(x, alpha_mod, nb, &mut b);
    if ncalc != nb {
        /* error input */
        if ncalc < 0 {
            // MATHLIB_WARNING4 -- just issue a warning
            ml_warning(ME_RANGE, "bessel_j");
        } else {
            // MATHLIB_WARNING2 -- precision lost
            ml_warning(ME_PRECISION, "bessel_j");
        }
    }
    b[(nb - 1) as usize]
}

/// C FFI wrapper for bessel_j
pub extern "C" fn bessel_j_c(x: f64, alpha: f64) -> f64 {
    bessel_j(x, alpha)
}

// =====================================================================
// bessel_j_ex: version accepting a pre-allocated work array
// =====================================================================

/// Modified version of bessel_j(), accepting a work array instead of allocating one.
/// Called from R via math_2b().
///
/// # Arguments
/// * `x` - Non-negative argument
/// * `alpha` - Order (may be negative)
/// * `bj` - Work array (must be large enough, typically nb elements)
///
/// # Returns
/// J_alpha(x)
pub fn bessel_j_ex(x: f64, alpha: f64, bj: &mut [f64]) -> f64 {
    // NaNs propagated correctly
    if isnan(x) || isnan(alpha) {
        return x + alpha;
    }
    if x < 0.0 {
        ml_warning(ME_RANGE, "bessel_j");
        return ML_NAN;
    }
    // ==> x >= 0 from now on
    let na = floor(alpha);
    if alpha < 0.0 {
        /* Using Abramowitz & Stegun  9.1.2 */
        let part1: f64 = if alpha - na == 0.5 {
            0.0
        } else {
            bessel_j_ex(x, -alpha, bj) * cospi(alpha)
        };
        let part2: f64 = if alpha == na {
            0.0
        } else {
            bessel_y_ex(x, -alpha, bj) * sinpi(alpha)
        };
        return part1 + part2;
    } else if alpha > 1e7 {
        ml_warning(ME_RANGE, "bessel_j");
        return ML_NAN;
    }
    let nb: i32 = 1 + (na as i32); /* nb-1 <= alpha < nb */
    let alpha_mod = alpha - ((nb - 1) as f64); // ==> alpha' in [0, 1)

    let ncalc = j_bessel(x, alpha_mod, nb, bj);
    if ncalc != nb {
        /* error input */
        if ncalc < 0 {
            ml_warning(ME_RANGE, "bessel_j");
        } else {
            ml_warning(ME_PRECISION, "bessel_j");
        }
    }
    bj[(nb - 1) as usize]
}

/// C FFI wrapper for bessel_j_ex
pub extern "C" fn bessel_j_ex_c(x: f64, alpha: f64, bj: *mut f64, nb: i32) -> f64 {
    if bj.is_null() || nb <= 0 {
        return ML_NAN;
    }
    let bj_slice = unsafe { std::slice::from_raw_parts_mut(bj, nb as usize) };
    bessel_j_ex(x, alpha, bj_slice)
}
