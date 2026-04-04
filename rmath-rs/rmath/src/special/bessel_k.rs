// Ported from R's nmath/bessel_k.c
//
// From http://www.netlib.org/specfun/rkbesl Fortran translated by f2c,...
//   Martin Maechler, ETH Zurich
//
// Computes modified Bessel functions of the third kind,
// K_nu(x), for non-negative argument x and non-negative order nu,
// with or without exponential scaling.

use crate::constants::*;
use crate::error::*;
use libm::*;

// Constants from bessel.h
const XMAX_BESS_K: f64 = 705.342; // maximal x for UNscaled answer
const SQXMIN_BESS_K: f64 = 1.49e-154; // sqrt(DBL_MIN)
const DBL_EPSILON: f64 = 2.220446049250313e-16;
const DBL_MAX: f64 = 1.7976931348623157e+308;
const DBL_MIN: f64 = 2.2250738585072014e-308;
const M_SQRT_2dPI: f64 = 0.79788456080286535587989211986876; // sqrt(2/pi)

// Mathematical constants
const A: f64 = 0.11593151565841244881; // LOG(2) - Euler's constant

// P, Q - Approximation for LOG(GAMMA(1+ALPHA))/ALPHA + Euler's constant
// Coefficients converted from hex to decimal and modified by W. J. Cody, 2/26/82
const P: [f64; 8] = [
    0.805629875690432845,
    20.4045500205365151,
    157.705605106676174,
    536.671116469207504,
    900.382759291288778,
    730.923886650660393,
    229.299301509425145,
    0.822467033424113231,
];

const Q: [f64; 7] = [
    29.4601986247850434,
    277.577868510221208,
    1206.70325591027438,
    2762.91444159791519,
    3443.74050506564618,
    2210.63190113378647,
    572.267338359892221,
];

// R, S - Approximation for (1-ALPHA*PI/SIN(ALPHA*PI))/(2.D0*ALPHA)
const R_COEFF: [f64; 5] = [
    -0.48672575865218401848,
    13.079485869097804016,
    -101.96490580880537526,
    347.65409106507813131,
    3.495898124521934782e-4,
];

const S_COEFF: [f64; 4] = [
    -25.579105509976461286,
    212.57260432226544008,
    -610.69018684944109624,
    422.69668805777760407,
];

// T - Approximation for SINH(Y)/Y
const T_COEFF: [f64; 6] = [
    1.6125990452916363814e-10,
    2.5051878502858255354e-8,
    2.7557319615147964774e-6,
    1.9841269840928373686e-4,
    0.0083333333333334751799,
    0.16666666666666666446,
];

const ESTM: [f64; 6] = [52.0583, 5.7607, 2.7782, 14.4303, 185.3004, 9.3715];
const ESTF: [f64; 7] = [41.8341, 7.1075, 6.4306, 42.511, 1.35633, 84.5096, 20.0];

/// min0(x, y) = min(x, y) for int-like semantics (C macro)
#[inline(always)]
fn min0(x: i32, y: i32) -> i32 {
    if x <= y { x } else { y }
}

/// max0(x, y) = max(x, y) for int-like semantics (C macro)
#[inline(always)]
fn max0(x: i32, y: i32) -> i32 {
    if x <= y { y } else { x }
}

/// Modified Bessel function of the third kind, K_alpha(x).
///
/// # Arguments
/// * `x` - Non-negative argument
/// * `alpha` - Order (non-negative, can be fractional)
/// * `expo` - Scaling type: 1.0 for unscaled K, 2.0 for exponentially scaled K*exp(x)
pub fn bessel_k(x: f64, alpha: f64, expo: f64) -> f64 {
    /* NaNs propagated correctly */
    if isnan(x) || isnan(alpha) {
        return x + alpha;
    }
    if x < 0.0 {
        ml_warning(ME_RANGE, "bessel_k");
        return ML_NAN;
    }
    let ize = expo as i32;
    let mut alpha = if alpha < 0.0 { -alpha } else { alpha };
    let nb = 1 + floor(alpha) as i32; /* nb-1 <= |alpha| < nb */
    alpha -= (nb - 1) as f64;

    let mut bk = vec![0.0; nb as usize];
    let mut ncalc: i32 = 0;
    k_bessel(x, alpha, nb, ize, &mut bk, &mut ncalc);

    if ncalc != nb {
        /* error input */
        if ncalc < 0 {
            eprintln!(
                "bessel_k({}): ncalc (={}) != nb (={}); alpha={}. Arg. out of range?\n",
                x, ncalc, nb, alpha
            );
        } else {
            eprintln!(
                "bessel_k({},nu={}): precision lost in result\n",
                x,
                alpha + (nb as f64) - 1.0
            );
        }
    }
    bk[(nb - 1) as usize]
}

/// Modified Bessel function of the third kind with user-supplied work array.
///
/// This is a modified version of bessel_k that accepts a work array
/// instead of allocating one.
///
/// # Arguments
/// * `x` - Non-negative argument
/// * `alpha` - Order (non-negative, can be fractional)
/// * `expo` - Scaling type: 1.0 for unscaled K, 2.0 for exponentially scaled K*exp(x)
/// * `bk` - Work array of sufficient length (at least `1 + floor(|alpha|)` elements)
pub fn bessel_k_ex(x: f64, alpha: f64, expo: f64, bk: &mut [f64]) -> f64 {
    /* NaNs propagated correctly */
    if isnan(x) || isnan(alpha) {
        return x + alpha;
    }
    if x < 0.0 {
        ml_warning(ME_RANGE, "bessel_k");
        return ML_NAN;
    }
    let ize = expo as i32;
    let mut alpha = if alpha < 0.0 { -alpha } else { alpha };
    let nb = 1 + floor(alpha) as i32; /* nb-1 <= |alpha| < nb */
    alpha -= (nb - 1) as f64;

    let mut ncalc: i32 = 0;
    k_bessel(x, alpha, nb, ize, bk, &mut ncalc);

    if ncalc != nb {
        /* error input */
        if ncalc < 0 {
            eprintln!(
                "bessel_k({}): ncalc (={}) != nb (={}); alpha={}. Arg. out of range?\n",
                x, ncalc, nb, alpha
            );
        } else {
            eprintln!(
                "bessel_k({},nu={}): precision lost in result\n",
                x,
                alpha + (nb as f64) - 1.0
            );
        }
    }
    bk[(nb - 1) as usize]
}

/// Internal routine: K_bessel
///
/// This routine calculates modified Bessel functions
/// of the third kind, K_(N+ALPHA) (X), for non-negative
/// argument X, and non-negative order N+ALPHA, with or without
/// exponential scaling.
fn k_bessel(x: f64, alpha: f64, nb: i32, ize: i32, bk: &mut [f64], ncalc: &mut i32) {
    let ex = x;
    let mut nu = alpha;
    *ncalc = min0(nb, 0) - 2;

    if !(nb > 0 && (0.0 <= nu && nu < 1.0) && (1 <= ize && ize <= 2)) {
        return;
    }

    if ex <= 0.0 || (ize == 1 && ex > XMAX_BESS_K) {
        if ex <= 0.0 {
            if ex < 0.0 {
                ml_warning(ME_RANGE, "K_bessel");
            }
            for i in 0..(nb as usize) {
                bk[i] = ML_POSINF;
            }
        } else {
            /* would only have underflow */
            for i in 0..(nb as usize) {
                bk[i] = 0.0;
            }
        }
        *ncalc = nb;
        return;
    }

    let mut k = 0;
    if nu < SQXMIN_BESS_K {
        nu = 0.0;
    } else if nu > 0.5 {
        k = 1;
        nu -= 1.0;
    }
    let mut twonu = nu + nu;
    let iend = nb + k - 1;
    let c = nu * nu;
    let mut d3 = -c;

    if ex <= 1.0 {
        /* ------------------------------------------------------------
        Calculation of P0 = GAMMA(1+ALPHA) * (2/X)**ALPHA
                       Q0 = GAMMA(1-ALPHA) * (X/2)**ALPHA
        ------------------------------------------------------------ */
        let mut d1 = 0.0;
        let mut d2 = P[0];
        let mut t1 = 1.0;
        let mut t2 = Q[0];
        let mut i = 2;
        while i <= 7 {
            d1 = c * d1 + P[(i - 1) as usize];
            d2 = c * d2 + P[i as usize];
            t1 = c * t1 + Q[(i - 1) as usize];
            t2 = c * t2 + Q[i as usize];
            i += 2;
        }
        d1 *= nu;
        t1 *= nu;
        let mut f1 = log(ex);
        let mut f0 = A + nu * (P[7] - nu * (d1 + d2) / (t1 + t2)) - f1;
        let mut q0 = exp(-nu * (A - nu * (P[7] + nu * (d1 - d2) / (t1 - t2)) - f1));
        f1 = nu * f0;
        let mut p0 = exp(f1);

        /* -----------------------------------------------------------
        Calculation of F0
        ----------------------------------------------------------- */
        let mut d1 = R_COEFF[4];
        let mut t1 = 1.0;
        for ii in 0..4 {
            d1 = c * d1 + R_COEFF[ii];
            t1 = c * t1 + S_COEFF[ii];
        }

        /* d2 := sinh(f1)/ nu = sinh(f1)/(f1/f0)
         *     = f0 * sinh(f1)/f1 */
        let mut d2;
        if fabs(f1) <= 0.5 {
            let f1sq = f1 * f1;
            d2 = 0.0;
            for ii in 0..6 {
                d2 = f1sq * d2 + T_COEFF[ii];
            }
            d2 = f0 + f0 * f1sq * d2;
        } else {
            d2 = sinh(f1) / nu;
        }
        f0 = d2 - nu * d1 / (t1 * p0);

        if ex <= 1e-10 {
            /* ---------------------------------------------------------
            X <= 1.0E-10
            Calculation of K(ALPHA,X) and X*K(ALPHA+1,X)/K(ALPHA,X)
            --------------------------------------------------------- */
            bk[0] = f0 + ex * f0;
            if ize == 1 {
                bk[0] -= ex * bk[0];
            }
            let mut ratio = p0 / f0;
            let c_local = ex * DBL_MAX;

            if k != 0 {
                /* ---------------------------------------------------
                Calculation of K(ALPHA,X)
                and  X*K(ALPHA+1,X)/K(ALPHA,X),  ALPHA >= 1/2
                --------------------------------------------------- */
                *ncalc = -1;
                if bk[0] >= c_local / ratio {
                    return;
                }
                bk[0] = ratio * bk[0] / ex;
                twonu += 2.0;
                ratio = twonu;
            }
            *ncalc = 1;
            if nb == 1 {
                return;
            }

            /* -----------------------------------------------------
            Calculate  K(ALPHA+L,X)/K(ALPHA+L-1,X),
            L = 1, 2, ... , NB-1
            ----------------------------------------------------- */
            *ncalc = -1;
            for i in 1..(nb as usize) {
                if ratio >= c_local {
                    return;
                }
                bk[i] = ratio / ex;
                twonu += 2.0;
                ratio = twonu;
            }
            *ncalc = 1;

            /* L420 */
            let mut i = *ncalc as usize;
            while i < nb as usize {
                bk[i] *= bk[i - 1];
                *ncalc += 1;
                i += 1;
            }
            return;
        } else {
            /* ------------------------------------------------------
            10^-10 < X <= 1.0
            ------------------------------------------------------ */
            let mut c_local = 1.0;
            let x2by4 = ex * ex / 4.0;
            p0 *= 0.5;
            q0 *= 0.5;
            let mut d1 = -1.0;
            let mut d2 = 0.0;
            let mut bk1 = 0.0;
            let mut bk2 = 0.0;
            let f1_sav = f0;
            let f2 = p0;

            loop {
                d1 += 2.0;
                d2 += 1.0;
                d3 += d1;
                c_local = x2by4 * c_local / d2;
                f0 = (d2 * f0 + p0 + q0) / d3;
                p0 /= d2 - nu;
                q0 /= d2 + nu;
                let t1 = c_local * f0;
                let t2 = c_local * (p0 - d2 * f0);
                bk1 += t1;
                bk2 += t2;

                if !(fabs(t1 / (f1_sav + bk1)) > DBL_EPSILON || fabs(t2 / (f2 + bk2)) > DBL_EPSILON)
                {
                    break;
                }
            }
            bk1 += f1_sav;
            bk2 = 2.0 * (f2 + bk2) / ex;
            let wminf;
            if ize == 2 {
                d1 = exp(ex);
                bk1 *= d1;
                bk2 *= d1;
            }
            wminf = ESTF[0] * ex + ESTF[1];

            /* Fall through to common forward recurrence section */
            k_bessel_forward(ex, nu, twonu, iend, k, ize, nb, bk, ncalc, bk1, bk2, wminf);
            return;
        }
    } else if DBL_EPSILON * ex > 1.0 {
        /* -------------------------------------------------
        X > 1./EPS
        ------------------------------------------------- */
        *ncalc = nb;
        let bk1 = 1.0 / (M_SQRT_2dPI * sqrt(ex));
        for i in 0..(nb as usize) {
            bk[i] = bk1;
        }
        return;
    } else {
        /* -------------------------------------------------------
        X > 1.0
        ------------------------------------------------------- */
        let twox = ex + ex;
        let mut blpha = 0.0;
        let mut ratio = 0.0;

        let wminf;
        let mut bk1;
        let bk2;

        if ex <= 4.0 {
            /* ----------------------------------------------------------
            Calculation of K(ALPHA+1,X)/K(ALPHA,X),  1.0 <= X <= 4.0
            ---------------------------------------------------------- */
            let mut d2 = trunc(ESTM[0] / ex + ESTM[1]);
            let m = d2 as i32;
            let mut d1 = d2 + d2;
            d2 -= 0.5;
            d2 *= d2;

            let mut i = 2;
            while i <= m {
                d1 -= 2.0;
                d2 -= d1;
                ratio = (d3 + d2) / (twox + d1 - ratio);
                i += 1;
            }

            /* -----------------------------------------------------------
            Calculation of I(|ALPHA|,X) and I(|ALPHA|+1,X) by backward
            recurrence and K(ALPHA,X) from the wronskian
            ----------------------------------------------------------- */
            let mut d2 = trunc(ESTM[2] * ex + ESTM[3]);
            let m = d2 as i32;
            let c_local = fabs(nu);
            let d3_loc = c_local + c_local;
            let d1_init = d3_loc - 1.0;
            let mut f1 = DBL_MIN;
            let mut f0 = (2.0 * (c_local + d2) / ex + 0.5 * ex / (c_local + d2 + 1.0)) * DBL_MIN;

            let mut i = 3;
            while i <= m {
                d2 -= 1.0;
                let mut f2 = (d3_loc + d2 + d2) * f0;
                blpha = (1.0 + d1_init / d2) * (f2 + blpha);
                f2 = f2 / ex + f1;
                f1 = f0;
                f0 = f2;
                i += 1;
            }
            f1 += (d3_loc + 2.0) * f0 / ex;

            let mut d1 = 0.0;
            let mut t1 = 1.0;
            for i in 1..=7 {
                d1 = c_local * d1 + P[(i - 1) as usize];
                t1 = c_local * t1 + Q[(i - 1) as usize];
            }
            let p0 = exp(c_local * (A + c_local * (P[7] - c_local * d1 / t1) - log(ex))) / ex;
            let f2 = (c_local + 0.5 - ratio) * f1 / ex;
            bk1 = p0 + (d3_loc * f0 - f2 + f0 + blpha) / (f2 + f1 + f0) * p0;
            if ize == 1 {
                bk1 *= exp(-ex);
            }
            wminf = ESTF[2] * ex + ESTF[3];
        } else {
            /* ---------------------------------------------------------
            Calculation of K(ALPHA,X) and K(ALPHA+1,X)/K(ALPHA,X), by
            backward recurrence, for  X > 4.0
            ---------------------------------------------------------- */
            let mut dm = trunc(ESTM[4] / ex + ESTM[5]);
            let m = dm as i32;
            let mut d2 = dm - 0.5;
            d2 *= d2;
            let mut d1 = dm + dm;

            let mut i = 2;
            while i <= m {
                dm -= 1.0;
                d1 -= 2.0;
                d2 -= d1;
                ratio = (d3 + d2) / (twox + d1 - ratio);
                blpha = (ratio + ratio * blpha) / dm;
                i += 1;
            }
            bk1 = 1.0 / ((M_SQRT_2dPI + M_SQRT_2dPI * blpha) * sqrt(ex));
            if ize == 1 {
                bk1 *= exp(-ex);
            }
            wminf = ESTF[4] * (ex - fabs(ex - ESTF[6])) + ESTF[5];
        }

        /* ---------------------------------------------------------
        Calculation of K(ALPHA+1,X)
        from K(ALPHA,X) and  K(ALPHA+1,X)/K(ALPHA,X)
        --------------------------------------------------------- */
        bk2 = bk1 + bk1 * (nu + 0.5 - ratio) / ex;

        /* Fall through to common forward recurrence section */
        k_bessel_forward(ex, nu, twonu, iend, k, ize, nb, bk, ncalc, bk1, bk2, wminf);
    }
}

/// Common forward recurrence section: compute K(ALPHA+I,X) for I = 0, 1, ..., NCALC-1
/// and ratios K(ALPHA+I,X)/K(ALPHA+I-1,X) for the remaining indices.
///
/// This corresponds to lines 489-559 of the original C code (the shared tail
/// after the three main computation branches).
fn k_bessel_forward(
    ex: f64,
    nu: f64,
    twonu: f64,
    iend: i32,
    k: i32,
    _ize: i32,
    nb: i32,
    bk: &mut [f64],
    ncalc: &mut i32,
    bk1: f64,
    bk2: f64,
    wminf: f64,
) {
    /*--------------------------------------------------------------------
    Calculation of 'NCALC', K(ALPHA+I,X),  I  =  0, 1, ... , NCALC-1,
    &     K(ALPHA+I,X)/K(ALPHA+I-1,X),  I = NCALC, NCALC+1, ... , NB-1
    -------------------------------------------------------------------*/
    let mut twonu = twonu;
    let mut bk1 = bk1;
    let mut bk2 = bk2;

    *ncalc = nb;
    bk[0] = bk1;
    if iend == 0 {
        return;
    }

    let mut j = 1 - k;
    if j >= 0 {
        bk[j as usize] = bk2;
    }

    if iend == 1 {
        return;
    }

    let m_end = min0((wminf - nu) as i32, iend);
    let mut ii = 0;
    let mut i = 2;
    while i <= m_end {
        let t1 = bk1;
        bk1 = bk2;
        twonu += 2.0;
        if ex < 1.0 {
            if bk1 >= DBL_MAX / twonu * ex {
                break;
            }
        } else {
            if bk1 / ex >= DBL_MAX / twonu {
                break;
            }
        }
        bk2 = twonu / ex * bk1 + t1;
        ii = i;
        j += 1;
        if j >= 0 {
            bk[j as usize] = bk2;
        }
        i += 1;
    }

    let m = ii;
    if m == iend {
        return;
    }
    let mut ratio = bk2 / bk1;
    let mplus1 = m + 1;
    *ncalc = -1;
    let mut i = mplus1;
    while i <= iend {
        twonu += 2.0;
        ratio = twonu / ex + 1.0 / ratio;
        j += 1;
        if j >= 1 {
            bk[j as usize] = ratio;
        } else {
            if bk2 >= DBL_MAX / ratio {
                return;
            }
            bk2 *= ratio;
        }
        i += 1;
    }
    *ncalc = max0(1, mplus1 - k);
    if *ncalc == 1 {
        bk[0] = bk2;
    }
    if nb == 1 {
        return;
    }

    /* L420 */
    let mut i = *ncalc as usize;
    while i < nb as usize {
        bk[i] *= bk[i - 1];
        *ncalc += 1;
        i += 1;
    }
}

// C FFI shims
mod imp {
    use super::*;

    #[unsafe(no_mangle)]
    pub extern "C" fn bessel_k_c(x: f64, alpha: f64, expo: f64) -> f64 {
        bessel_k(x, alpha, expo)
    }
}
