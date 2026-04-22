#![allow(clippy::neg_cmp_op_on_partial_ord)]
// Ported from R's nmath/bessel_y.c
//
// Original by W. J. Cody, Applied Mathematics Division,
// Argonne National Laboratory, Argonne, IL 60439.
//
// From http://www.netlib.org/specfun/rybesl
//   Fortran translated by f2c, Martin Maechler, ETH Zurich
//
// Computes Bessel functions of the second kind, Y_nu(x),
// for non-negative argument x and non-negative order nu.

use crate::constants::*;
use crate::error::*;
use crate::special::bessel_j::{bessel_j, bessel_j_ex};
use crate::special::cospi::{cospi, sinpi};
use libm::*;

// =====================================================================
// Constants from bessel.h
// =====================================================================

const XLRG_BESS_Y: f64 = 1e8;
const THRESH_BESS_Y: f64 = 16.0;
const M_EPS_SINC: f64 = 2.149119e-8;

const DBL_EPSILON: f64 = 2.220446049250313e-16;
const DBL_MAX: f64 = 1.7976931348623157e+308;
const DBL_MIN: f64 = 2.2250738585072014e-308;

const M_PI: f64 = 3.14159265358979323846264338327950288;
const M_SQRT_2DPI: f64 = 0.79788456080286535587989211986876; // sqrt(2/pi)
const M_1_PI: f64 = 0.31830988618379067153776752674503; // 1/pi
const M_PI_2: f64 = 1.57079632679489661923132169163975; // pi/2

/// min0(x, y) from C code: min of two ints, treating negative as 0.
#[inline]
fn min0(x: i32, y: i32) -> i32 {
    let a = if x < 0 { 0 } else { x };
    let b = if y < 0 { 0 } else { y };
    if a <= b { a } else { b }
}

// =====================================================================
// Internal Y_bessel
// =====================================================================

/// Calculates Bessel functions Y_{n+alpha}(x) for non-negative argument x,
/// and non-negative order n+alpha, n = 0, 1, ..., nb-1.
///
/// # Parameters
/// - `x`: Non-negative argument
/// - `alpha`: Fractional part of order, 0 <= ALPHA < 1
/// - `nb`: Number of functions to calculate, nb >= 1
/// - `by`: Output vector (0-indexed), length nb
///
/// # Returns
/// `ncalc`: Number of orders successfully calculated
fn y_bessel(x: f64, alpha: f64, nb: i32, by: &mut [f64]) -> i32 {
    // ----------------------------------------------------------------------
    //  Mathematical constants
    //    FIVPI = 5*PI
    //    PIM5 = 5*PI - 15
    // ----------------------------------------------------------------------
    let fivpi: f64 = 15.707963267948966192;
    let pim5: f64 = 0.70796326794896619231;

    // ---------------------------------------------------------------
    //  Coefficients for Chebyshev polynomial expansion of
    //  1/gamma(1-x), abs(x) <= .5
    // ---------------------------------------------------------------
    let ch: [f64; 21] = [
        -6.7735241822398840964e-24,
        -6.1455180116049879894e-23,
        2.9017595056104745456e-21,
        1.3639417919073099464e-19,
        2.3826220476859635824e-18,
        -9.0642907957550702534e-18,
        -1.4943667065169001769e-15,
        -3.3919078305362211264e-14,
        -1.7023776642512729175e-13,
        9.1609750938768647911e-12,
        2.4230957900482704055e-10,
        1.7451364971382984243e-9,
        -3.3126119768180852711e-8,
        -8.6592079961391259661e-7,
        -4.9717367041957398581e-6,
        7.6309597585908126618e-5,
        0.0012719271366545622927,
        0.0017063050710955562222,
        -0.07685284084478667369,
        -0.28387654227602353814,
        0.92187029365045265648,
    ];

    // Local variables
    let mut i: i32;
    let mut k: i32;
    let mut na: i32;

    let mut alfa: f64;
    let mut div: f64;
    let ddiv: f64;
    let mut even: f64;
    let gamma: f64;
    let mut term: f64;
    let mut cosmu: f64;
    let mut sinmu: f64;
    let mut b: f64;
    let mut c: f64;
    let mut d: f64;
    let mut e: f64;
    let mut f: f64;
    let mut g: f64;
    let mut h: f64;
    let mut p: f64;
    let mut q: f64;
    let mut r: f64;
    let mut s: f64;
    let mut d1: f64;
    let mut d2: f64;
    let mut q0: f64;
    let pa: f64;
    let pa1: f64;
    let qa: f64;
    let qa1: f64;
    let mut en: f64;
    let mut en1: f64;
    let mut nu: f64;
    let ex: f64;
    let mut ya: f64;
    let mut ya1: f64;
    let twobyx: f64;
    let den: f64;
    let mut odd: f64;
    let mut aye: f64;
    let mut dmu: f64;
    let mut x2: f64;
    let xna: f64;

    en1 = 0.0; /* -Wall */
    ya = 0.0;
    ya1 = 0.0;

    ex = x;
    nu = alpha;
    if nb > 0 && 0.0 <= nu && nu < 1.0 {
        if ex < DBL_MIN || ex > XLRG_BESS_Y {
            /* Warning is not really appropriate, give
             * proper limit:
             * ML_WARNING(ME_RANGE, "Y_bessel"); */
            let ncalc = nb;
            if ex > XLRG_BESS_Y {
                by[0] = 0.0; /* was ML_POSINF */
            } else if ex < DBL_MIN {
                by[0] = ML_NEGINF;
            }
            for ii in 0..(nb as usize) {
                by[ii] = by[0];
            }
            return ncalc;
        }
        xna = trunc(nu + 0.5);
        na = xna as i32;
        if na == 1 {
            /* <==>  .5 <= *alpha < 1  <==>  -5. <= nu < 0 */
            nu -= xna;
        }
        if nu == -0.5 {
            p = M_SQRT_2DPI / sqrt(ex);
            ya = p * sin(ex);
            ya1 = -p * cos(ex);
        } else if ex < 3.0 {
            /* -------------------------------------------------------------
            Use Temme's scheme for small X
            ------------------------------------------------------------- */
            b = ex * 0.5;
            d = -log(b);
            f = nu * d;
            e = pow(b, -nu);
            if fabs(nu) < M_EPS_SINC {
                c = M_1_PI;
            } else {
                c = nu / sinpi(nu);
            }

            /* ------------------------------------------------------------
            Computation of sinh(f)/f
            ------------------------------------------------------------ */
            if fabs(f) < 1.0 {
                x2 = f * f;
                en = 19.0;
                s = 1.0;
                i = 1;
                while i <= 9 {
                    s = s * x2 / en / (en - 1.0) + 1.0;
                    en -= 2.0;
                    i += 1;
                }
            } else {
                s = (e - 1.0 / e) * 0.5 / f;
            }
            /* --------------------------------------------------------
            Computation of 1/gamma(1-a) using Chebyshev polynomials */
            x2 = nu * nu * 8.0;
            aye = ch[0];
            even = 0.0;
            alfa = ch[1];
            odd = 0.0;
            i = 3;
            while i <= 19 {
                even = -(aye + aye + even);
                aye = -even * x2 - aye + ch[(i - 1) as usize];
                odd = -(alfa + alfa + odd);
                alfa = -odd * x2 - alfa + ch[i as usize];
                i += 2;
            }
            even = (even * 0.5 + aye) * x2 - aye + ch[20];
            odd = (odd + alfa) * 2.0;
            gamma = odd * nu + even;
            /* End of computation of 1/gamma(1-a)
            ----------------------------------------------------------- */
            g = e * gamma;
            e = (e + 1.0 / e) * 0.5;
            f = 2.0 * c * (odd * e + even * s * d);
            e = nu * nu;
            p = g * c;
            q = M_1_PI / g;
            c = nu * M_PI_2;
            if fabs(c) < M_EPS_SINC {
                r = 1.0;
            } else {
                r = sinpi(nu / 2.0) / c;
            }

            r = M_PI * c * r * r;
            c = 1.0;
            d = -b * b;
            h = 0.0;
            ya = f + r * q;
            ya1 = p;
            en = 1.0;

            loop {
                if fabs(g / (1.0 + fabs(ya))) + fabs(h / (1.0 + fabs(ya1))) <= DBL_EPSILON {
                    break;
                }
                f = (f * en + p + q) / (en * en - e);
                c *= d / en;
                p /= en - nu;
                q /= en + nu;
                g = c * (f + r * q);
                h = c * p - en * g;
                ya += g;
                ya1 += h;
                en += 1.0;
            }
            ya = -ya;
            ya1 = -ya1 / b;
        } else if ex < THRESH_BESS_Y {
            /* --------------------------------------------------------------
            Use Temme's scheme for moderate X :  3 <= x < 16
            -------------------------------------------------------------- */
            c = (0.5 - nu) * (0.5 + nu);
            b = ex + ex;
            e = ex * M_1_PI * cospi(nu) / DBL_EPSILON;
            e *= e;
            p = 1.0;
            q = -ex;
            r = 1.0 + ex * ex;
            s = r;
            en = 2.0;
            loop {
                if !(r * en * en < e) {
                    break;
                }
                en1 = en + 1.0;
                d = (en - 1.0 + c / en) / s;
                p = (en + en - p * d) / en1;
                q = (-b + q * d) / en1;
                s = p * p + q * q;
                r *= s;
                en = en1;
            }
            f = p / s;
            p = f;
            g = -q / s;
            q = g;
            // L220:
            loop {
                en -= 1.0;
                if !(en > 0.0) {
                    break;
                }
                r = en1 * (2.0 - p) - 2.0;
                s = b + en1 * q;
                d = (en - 1.0 + c / en) / (r * r + s * s);
                p = d * r;
                q = d * s;
                e = f + 1.0;
                f = p * e - g * q;
                g = q * e + p * g;
                en1 = en;
            }
            f += 1.0;
            d = f * f + g * g;
            pa = f / d;
            qa = -g / d;
            d = nu + 0.5 - p;
            q += ex;
            pa1 = (pa * q - qa * d) / ex;
            qa1 = (qa * q + pa * d) / ex;
            b = ex - M_PI_2 * (nu + 0.5);
            c = cos(b);
            s = sin(b);
            d = M_SQRT_2DPI / sqrt(ex);
            ya = d * (pa * s + qa * c);
            ya1 = d * (qa1 * s - pa1 * c);
        } else {
            /* x > thresh_BESS_Y
            ----------------------------------------------------------
            Use Campbell's asymptotic scheme.
            ---------------------------------------------------------- */
            na = 0;
            d1 = trunc(ex / fivpi);
            i = d1 as i32;
            dmu = ex - 15.0 * d1 - d1 * pim5 - (alpha + 0.5) * M_PI_2;
            if i % 2 == 0 {
                cosmu = cos(dmu);
                sinmu = sin(dmu);
            } else {
                cosmu = -cos(dmu);
                sinmu = -sin(dmu);
            }
            ddiv = 8.0 * ex;
            dmu = alpha;
            den = sqrt(ex);
            k = 1;
            while k <= 2 {
                p = cosmu;
                cosmu = sinmu;
                sinmu = -p;
                d1 = (2.0 * dmu - 1.0) * (2.0 * dmu + 1.0);
                d2 = 0.0;
                div = ddiv;
                p = 0.0;
                q = 0.0;
                q0 = d1 / div;
                term = q0;
                i = 2;
                while i <= 20 {
                    d2 += 8.0;
                    d1 -= d2;
                    div += ddiv;
                    term = -term * d1 / div;
                    p += term;
                    d2 += 8.0;
                    d1 -= d2;
                    div += ddiv;
                    term *= d1 / div;
                    q += term;
                    if fabs(term) <= DBL_EPSILON {
                        break;
                    }
                    i += 1;
                }
                p += 1.0;
                q += q0;
                if k == 1 {
                    ya = M_SQRT_2DPI * (p * cosmu - q * sinmu) / den;
                } else {
                    ya1 = M_SQRT_2DPI * (p * cosmu - q * sinmu) / den;
                }
                dmu += 1.0;
                k += 1;
            }
        }
        if na == 1 {
            h = 2.0 * (nu + 1.0) / ex;
            if h > 1.0 && fabs(ya1) > DBL_MAX / h {
                h = 0.0;
                ya = 0.0;
            }
            h = h * ya1 - ya;
            ya = ya1;
            ya1 = h;
        }

        /* ---------------------------------------------------------------
        Now have first one or two Y's
        --------------------------------------------------------------- */
        by[0] = ya;
        let mut ncalc = 1;
        if nb > 1 {
            by[1] = ya1;
            if ya1 != 0.0 {
                aye = 1.0 + alpha;
                twobyx = 2.0 / ex;
                ncalc = 2;
                i = 2;
                while i < nb {
                    if twobyx < 1.0 {
                        if fabs(by[(i - 1) as usize]) * twobyx >= DBL_MAX / aye {
                            // goto L450
                            break;
                        }
                    } else {
                        if fabs(by[(i - 1) as usize]) >= DBL_MAX / aye / twobyx {
                            // goto L450
                            break;
                        }
                    }
                    by[i as usize] = twobyx * aye * by[(i - 1) as usize] - by[(i - 2) as usize];
                    aye += 1.0;
                    ncalc += 1;
                    i += 1;
                }
            }
        }
        // L450:
        for ii in (ncalc as usize)..(nb as usize) {
            by[ii] = ML_NEGINF; /* was 0 */
        }

        return ncalc;
    }

    // Error return -- X, NB, or ALPHA = nu is out of range
    by[0] = 0.0;
    min0(nb, 0) - 1
}

// =====================================================================
// Public API
// =====================================================================

/// Bessel function of the second kind, Y_nu(x).
#[must_use]
///
/// Ported from R's bessel_y() in bessel_y.c.
///
/// # Arguments
/// * `x` - Non-negative argument
/// * `alpha` - Order (may be negative)
///
/// # Returns
/// Y_alpha(x)
pub fn bessel_y(x: f64, alpha: f64) -> f64 {
    /* NaNs propagated correctly */
    if isnan(x) || isnan(alpha) {
        return x + alpha;
    }
    if x < 0.0 {
        ml_warning(ME_RANGE, "bessel_y");
        return ML_NAN;
    }
    let na = floor(alpha);
    if alpha < 0.0 {
        /* Using Abramowitz & Stegun  9.1.2
         * this may not be quite optimal (CPU and accuracy wise) */
        let part1: f64 = if alpha - na == 0.5 {
            0.0
        } else {
            bessel_y(x, -alpha) * cospi(alpha)
        };
        let part2: f64 = if alpha == na {
            0.0
        } else {
            bessel_j(x, -alpha) * sinpi(alpha)
        };
        return part1 - part2;
    } else if alpha > 1e7 {
        ml_warning(ME_RANGE, "bessel_y");
        return ML_NAN;
    }
    let nb = 1 + (na as i32); /* nb-1 <= alpha < nb */
    let alpha_mod = alpha - ((nb - 1) as f64);
    let mut by = vec![0.0; nb as usize];
    let ncalc = y_bessel(x, alpha_mod, nb, &mut by);
    if ncalc != nb {
        /* error input */
        if ncalc == -1 {
            return ML_POSINF;
        } else if ncalc < -1 {
            // MATHLIB_WARNING4 -- precision warning, no return
            let _ = (x, ncalc, nb, alpha);
        } else {
            /* ncalc >= 0 -- precision lost */
            let _ = (x, alpha + (nb as f64) - 1.0);
        }
    }
    by[(nb - 1) as usize]
}

/// C FFI wrapper for bessel_y
#[must_use]
pub fn bessel_y_c(x: f64, alpha: f64) -> f64 {
    bessel_y(x, alpha)
}

// =====================================================================
// bessel_y_ex: version accepting a pre-allocated work array
// =====================================================================

/// Modified version of bessel_y(), accepting a work array instead of allocating one.
#[must_use]
///
/// # Arguments
/// * `x` - Non-negative argument
/// * `alpha` - Order (may be negative)
/// * `by` - Work array (must be large enough)
///
/// # Returns
/// Y_alpha(x)
pub fn bessel_y_ex(x: f64, alpha: f64, by: &mut [f64]) -> f64 {
    /* NaNs propagated correctly */
    if isnan(x) || isnan(alpha) {
        return x + alpha;
    }
    if x < 0.0 {
        ml_warning(ME_RANGE, "bessel_y");
        return ML_NAN;
    }
    let na = floor(alpha);
    if alpha < 0.0 {
        /* Using Abramowitz & Stegun  9.1.2
         * this may not be quite optimal (CPU and accuracy wise) */
        let part1: f64 = if alpha - na == 0.5 {
            0.0
        } else {
            bessel_y_ex(x, -alpha, by) * cospi(alpha)
        };
        let part2: f64 = if alpha == na {
            0.0
        } else {
            bessel_j_ex(x, -alpha, by) * sinpi(alpha)
        };
        return part1 - part2;
    } else if alpha > 1e7 {
        ml_warning(ME_RANGE, "bessel_y");
        return ML_NAN;
    }
    let nb = 1 + (na as i32); /* nb-1 <= alpha < nb */
    let alpha_mod = alpha - ((nb - 1) as f64);
    let ncalc = y_bessel(x, alpha_mod, nb, by);
    if ncalc != nb {
        /* error input */
        if ncalc == -1 {
            return ML_POSINF;
        } else if ncalc < -1 {
            // MATHLIB_WARNING4 -- precision warning
            let _ = (x, ncalc, nb, alpha);
        } else {
            /* ncalc >= 0 -- precision lost */
            let _ = (x, alpha + (nb as f64) - 1.0);
        }
    }
    by[(nb - 1) as usize]
}

/// C FFI wrapper for bessel_y_ex
#[must_use]
pub fn bessel_y_ex_c(x: f64, alpha: f64, by: *mut f64, nb: i32) -> f64 {
    if by.is_null() || nb <= 0 {
        return ML_NAN;
    }
    let by_slice = unsafe { std::slice::from_raw_parts_mut(by, nb as usize) };
    bessel_y_ex(x, alpha, by_slice)
}
