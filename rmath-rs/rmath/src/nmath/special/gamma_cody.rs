//! Port of R's nmath/gamma_cody.c -- Cody's Gamma function.
//!
//! Original copyright:
//!   From http://www.netlib.org/specfun/gamma
//!   Fortran translated by f2c, Martin Maechler, ETH Zurich.
//!   Was part of ribesl (Bessel I(.)).
//!
//! This routine calculates the GAMMA function for a float argument X.
//! Computation is based on an algorithm outlined in:
//!   [1] "An Overview of Software Development for Special Functions",
//!       W. J. Cody, Lecture Notes in Mathematics, 506,
//!       Numerical Analysis Dundee, 1975, G. A. Watson (ed.),
//!       Springer Verlag, Berlin, 1976.
//!   [2] Computer Approximations, Hart, Et. Al., Wiley and sons, New York, 1968.
//!
//! Authors: W. J. Cody and L. Stoltz
//! Applied Mathematics Division, Argonne National Laboratory
//! Latest modification: October 12, 1989
//!
//! Used in bessel_i.c and bessel_j.c.

use crate::nmath::constants::*;
use crate::nmath::special::cospi::sinpi;
use libm::{exp, log, trunc};

/// Cody's Gamma function implementation.
///
/// Ported from R's `Rf_gamma_cody(double x)` in nmath/gamma_cody.c.
///
/// Provides a high-accuracy Gamma function (at least 20 significant decimal digits)
/// using rational approximations. Returns ML_POSINF for singularities or overflow.
pub unsafe fn Rf_gamma_cody(x: f64) -> f64 {
    const SQRTPI: f64 = 0.9189385332046727417803297;
    const XBIG: f64 = 171.624;

    // Numerator and denominator coefficients for rational minimax
    // approximation over (1,2).
    const P: [f64; 8] = [
        -1.71618513886549492533811,
        24.7656508055759199108314,
        -379.804256470945635097577,
        629.331155312818442661052,
        866.966202790413211295064,
        -31451.2729688483675254357,
        -36144.4134186911729807069,
        66456.1438202405440627855,
    ];
    const Q: [f64; 8] = [
        -30.8402300119738975254353,
        315.350626979604161529144,
        -1015.15636749021914166146,
        -3107.77167157231109440444,
        22538.1184209801510330112,
        4755.84627752788110767815,
        -134659.959864969306392456,
        -115132.259675553483497211,
    ];

    // Coefficients for minimax approximation over (12, INF).
    const C: [f64; 7] = [
        -0.001910444077728,
        8.4171387781295e-4,
        -5.952379913043012e-4,
        7.93650793500350248e-4,
        -0.002777777777777681622553,
        0.08333333333333333331554247,
        0.0057083835261,
    ];

    let mut parity: i32 = 0;
    let mut fact: f64 = 1.0;
    let mut n: i32 = 0;
    let mut y = x;

    if y <= 0.0 {
        // Argument is negative
        y = -x;
        let yi = trunc(y);
        let res = y - yi;
        if res != 0.0 {
            // Check if yi is odd
            if trunc(yi * 0.5) * 2.0 != yi {
                parity = 1;
            }
            fact = -std::f64::consts::PI / sinpi(res);
            y += 1.0;
        } else {
            return ML_POSINF;
        }
    }

    // Argument is positive
    if y < f64::EPSILON {
        // Argument < EPS
        if y >= f64::MIN_POSITIVE {
            return 1.0 / y;
        } else {
            return ML_POSINF;
        }
    } else if y < 12.0 {
        let yi = y;
        let z = if y < 1.0 {
            // EPS < argument < 1
            let z = y;
            y += 1.0;
            z
        } else {
            // 1 <= argument < 12, reduce argument if necessary
            n = y as i32 - 1;
            y -= n as f64;
            y - 1.0
        };

        // Evaluate approximation for 1. < argument < 2.
        let mut xnum = 0.0_f64;
        let mut xden = 1.0_f64;
        for i in 0..8 {
            xnum = (xnum + P[i]) * z;
            xden = xden * z + Q[i];
        }
        let mut res = xnum / xden + 1.0;

        if yi < y {
            // Adjust result for case 0. < argument < 1.
            res /= yi;
        } else if yi > y {
            // Adjust result for case 2. < argument < 12.
            for _ in 0..n {
                res *= y;
                y += 1.0;
            }
        }

        if parity != 0 { -res } else { res }
    } else {
        // Evaluate for argument >= 12.
        if y <= XBIG {
            let ysq = y * y;
            let mut sum = C[6];
            for i in 0..6 {
                sum = sum / ysq + C[i];
            }
            sum = sum / y - y + SQRTPI;
            sum += (y - 0.5) * log(y);
            let mut res = exp(sum);
            if parity != 0 {
                res = -res;
            }
            if fact != 1.0 {
                res = fact / res;
            }
            res
        } else {
            if parity != 0 { -ML_POSINF } else { ML_POSINF }
        }
    }
}
