//! Port of R's src/main/machar.c — computes machine floating-point constants.
//!
//! Original: Algorithm 665, collected algorithms from ACM.
//! Published in Transactions on Mathematical Software, vol. 14, no. 4, pp. 303-311.
//!
//! Author: W. J. Cody, Argonne National Laboratory.
//! Latest revision - April 20, 1987.
//!
//! This file provides `R_machar()` which computes ALL machine constants at once.
//! The C version is a template instantiated for `double` in platform.c; here we
//! provide the double instantiation directly.

#![allow(non_snake_case)]

use std::os::raw::c_int;

/// Computes machine floating-point constants for `double` precision.
///
/// Ported from R's src/main/machar.c (double instantiation).
///
/// # Parameters (output pointers):
/// - `ibeta`: radix for the floating-point representation
/// - `it`: number of base ibeta digits in the significand
/// - `irnd`: 0=chops, 1=rounds (not IEEE), 2=IEEE, 3/4/5=with partial underflow
/// - `ngrd`: number of guard digits for multiplication with truncating arithmetic
/// - `machep`: largest negative integer such that 1.0+ibeta^machep != 1.0
/// - `negep`: largest negative integer such that 1.0-ibeta^negep != 1.0
/// - `iexp`: number of bits reserved for the exponent (including bias/sign)
/// - `minexp`: largest negative integer such that ibeta^minexp is normalized
/// - `maxexp`: smallest positive power of beta that overflows
/// - `eps`: smallest positive number such that 1.0+eps != 1.0
/// - `epsneg`: small positive number such that 1.0-epsneg != 1.0
/// - `xmin`: smallest non-vanishing normalized floating-point power of the radix
/// - `xmax`: largest finite floating-point number
pub unsafe fn R_machar(
    ibeta: *mut c_int,
    it: *mut c_int,
    irnd: *mut c_int,
    ngrd: *mut c_int,
    machep: *mut c_int,
    negep: *mut c_int,
    iexp: *mut c_int,
    minexp: *mut c_int,
    maxexp: *mut c_int,
    eps: *mut f64,
    epsneg: *mut f64,
    xmin: *mut f64,
    xmax: *mut f64,
) {
    unsafe {
        let one: f64 = 1.0;
        let two: f64 = one + one;
        let zero: f64 = one - one;

        // Determine ibeta, beta ala Malcolm.
        let mut a: f64 = one;
        loop {
            a = a + a;
            let temp = a + one;
            let temp1 = temp - a;
            if !(temp1 - one == zero) {
                break;
            }
        }
        *ibeta = std::f64::RADIX as c_int;
        let beta: f64 = *ibeta as f64;

        // Determine it, irnd.
        *it = 0;
        let mut b: f64 = one;
        loop {
            *it += 1;
            b = b * beta;
            let temp = b + one;
            let temp1 = temp - b;
            if !(temp1 - one == zero) {
                break;
            }
        }
        *irnd = 0;
        let betah: f64 = beta / two;
        let tempa: f64;
        {
            let temp = a + betah;
            if temp - a != zero {
                *irnd = 1;
            }
            tempa = a + beta;
            let temp = tempa + betah;
            if *irnd == 0 && temp - tempa != zero {
                *irnd = 2;
            }
        }

        // Determine negep, epsneg.
        *negep = *it + 3;
        let betain: f64 = one / beta;
        a = one;
        for _ in 1..=*negep {
            a = a * betain;
        }
        let b_orig: f64 = a;
        loop {
            let temp = one - a;
            if temp - one != zero {
                break;
            }
            a = a * beta;
            *negep -= 1;
        }
        *negep = -*negep;
        *epsneg = a;
        if *ibeta != 2 && *irnd != 0 {
            a = (a * (one + a)) / two;
            let temp = one - a;
            if temp - one != zero {
                *epsneg = a;
            }
        }

        // Determine machep, eps.
        *machep = -*it - 3;
        a = b_orig;
        loop {
            let temp = one + a;
            if temp - one != zero {
                break;
            }
            a = a * beta;
            *machep += 1;
        }
        *eps = a;
        if *ibeta != 2 && *irnd != 0 {
            a = (a * (one + a)) / two;
            let temp = one + a;
            if temp - one != zero {
                *eps = a;
            }
        }

        // Determine ngrd.
        *ngrd = 0;
        let temp = one + *eps;
        if *irnd == 0 && temp * one - one != zero {
            *ngrd = 1;
        }

        // Determine iexp, minexp, xmin.
        let mut i: c_int = 0;
        let mut k: c_int = 1;
        let mut z: f64 = betain;
        let t: f64 = one + *eps;
        let mut nxres: c_int = 0;
        let mut y: f64;
        loop {
            y = z;
            z = y * y;

            // Check for underflow here.
            a = z * one;
            let temp = z * t;
            if a + a == zero || z.abs() >= y {
                break;
            }
            let temp1 = temp * betain;
            if temp1 * beta == z {
                break;
            }
            i += 1;
            k += k;
        }

        let mut mx: c_int;
        if *ibeta != 10 {
            *iexp = i + 1;
            mx = k + k;
        } else {
            // This segment is for decimal machines only.
            *iexp = 2;
            let mut iz: c_int = *ibeta;
            while k >= iz {
                iz = iz * *ibeta;
                *iexp += 1;
            }
            mx = iz + iz - 1;
        }

        // Loop to determine minexp, xmin.
        loop {
            *xmin = y;
            y = y * betain;

            // Check for underflow here.
            a = y * one;
            let temp = y * t;
            if a + a == zero || y.abs() >= *xmin {
                break;
            }
            k += 1;
            let temp1 = temp * betain;
            if !(temp1 * beta != y) {
                nxres = 3;
                *xmin = y;
            }
        }
        *minexp = -k;

        // Determine maxexp, xmax.
        if mx <= k + k - 3 && *ibeta != 10 {
            mx = mx + mx;
            *iexp += 1;
        }
        *maxexp = mx + *minexp;

        // Adjust irnd to reflect partial underflow.
        *irnd += nxres;

        // Adjust for ieee-style machines.
        if *irnd == 2 || *irnd == 5 {
            *maxexp -= 2;
        }

        // Adjust for non-ieee machines with partial underflow.
        if *irnd == 3 || *irnd == 4 {
            *maxexp -= *it;
        }

        // Adjust for machines with implicit leading bit in binary
        // significand, and machines with radix point at extreme
        // right of significand.
        i = *maxexp + *minexp;
        if *ibeta == 2 && i == 0 {
            *maxexp -= 1;
        }
        if i > 20 {
            *maxexp -= 1;
        }
        if a != y {
            *maxexp -= 2;
        }
        *xmax = one - *epsneg;
        if *xmax * one != *xmax {
            *xmax = one - beta * *epsneg;
        }
        *xmax = *xmax / (beta * beta * beta * *xmin);
        i = *maxexp + *minexp + 3;
        if i > 0 {
            for _ in 1..=i {
                if *ibeta == 2 {
                    *xmax = *xmax + *xmax;
                } else {
                    *xmax = *xmax * beta;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_machar_basic() {
        unsafe {
            let mut ibeta: c_int = 0;
            let mut it: c_int = 0;
            let mut irnd: c_int = 0;
            let mut ngrd: c_int = 0;
            let mut machep: c_int = 0;
            let mut negep: c_int = 0;
            let mut iexp: c_int = 0;
            let mut minexp: c_int = 0;
            let mut maxexp: c_int = 0;
            let mut eps: f64 = 0.0;
            let mut epsneg: f64 = 0.0;
            let mut xmin: f64 = 0.0;
            let mut xmax: f64 = 0.0;

            R_machar(
                &mut ibeta,
                &mut it,
                &mut irnd,
                &mut ngrd,
                &mut machep,
                &mut negep,
                &mut iexp,
                &mut minexp,
                &mut maxexp,
                &mut eps,
                &mut epsneg,
                &mut xmin,
                &mut xmax,
            );

            // IEEE 754 double precision should have:
            assert_eq!(ibeta, 2, "ibeta should be 2 for IEEE 754");
            assert_eq!(it, 53, "it should be 53 for IEEE 754 double");
            // irnd should be 2 (IEEE rounding) or 5 (IEEE with partial underflow)
            assert!(
                irnd == 2 || irnd == 5,
                "irnd should be 2 or 5 for IEEE 754, got {}",
                irnd
            );
            assert_eq!(iexp, 11, "iexp should be 11 for IEEE 754 double");

            // Verify eps matches f64::EPSILON
            assert!(
                (eps - f64::EPSILON).abs() < 1e-20,
                "eps should match f64::EPSILON: eps={} EPSILON={}",
                eps,
                f64::EPSILON
            );

            // xmin should be positive and very small
            assert!(xmin > 0.0, "xmin should be positive");
            assert!(xmin < 1e-300, "xmin should be very small");

            // xmax should be very large (note: C comment says xmax may not be
            // the absolute largest number on some machines)
            assert!(xmax > 1e300, "xmax should be very large");
        }
    }
}
