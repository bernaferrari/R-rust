#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::os::raw::c_int;

/// Computes floating-point machine constants.
///
/// Algorithm 665, collected algorithms from ACM.
/// This work published in Transactions on Mathematical Software,
/// vol. 14, no. 4, pp. 303-311.
///
/// Ported from R's src/main/machar.c (double instantiation).
#[allow(clippy::eq_op)]
pub unsafe fn machar(
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
    let mut a: f64;
    let mut b: f64;
    let beta: f64;
    let betain: f64;
    let betah: f64;
    let one: f64;
    let t: f64;
    let mut temp: f64;
    let tempa: f64;
    let mut temp1: f64 = 0.0;
    let two: f64;
    let mut y: f64;
    let mut z: f64;
    let zero: f64;

    let mut i: c_int;
    let mut iz: c_int;
    let mut j: c_int;
    let mut k: c_int;
    let mut mx: c_int;
    let mut nxres: c_int;

    one = 1.0;
    two = one + one;
    zero = one - one;

    /* determine ibeta, beta ala malcolm. */
    a = one;
    loop {
        a = a + a;
        temp = a + one;
        temp1 = temp - a;
        if temp1 - one != zero {
            break;
        }
    }
    *ibeta = f64::RADIX as c_int;
    beta = f64::from(f64::RADIX);

    /* determine it, irnd */

    *it = 0;
    b = one;
    loop {
        *it += 1;
        b *= beta;
        temp = b + one;
        temp1 = temp - b;
        if temp1 - one != zero {
            break;
        }
    }
    *irnd = 0;
    betah = beta / two;
    temp = a + betah;
    if temp - a != zero {
        *irnd = 1;
    }
    tempa = a + beta;
    temp = tempa + betah;
    if *irnd == 0 && temp - tempa != zero {
        *irnd = 2;
    }

    /* determine negep, epsneg */

    *negep = *it + 3;
    betain = one / beta;
    a = one;
    i = 1;
    while i <= *negep {
        a *= betain;
        i += 1;
    }
    b = a;
    loop {
        temp = one - a;
        if temp - one != zero {
            break;
        }
        a *= beta;
        *negep -= 1;
    }
    *negep = -*negep;
    *epsneg = a;
    if *ibeta != 2 && *irnd != 0 {
        a = (a * (one + a)) / two;
        temp = one - a;
        if temp - one != zero {
            *epsneg = a;
        }
    }

    /* determine machep, eps */

    *machep = -*it - 3;
    a = b;
    loop {
        temp = one + a;
        if temp - one != zero {
            break;
        }
        a *= beta;
        *machep += 1;
    }
    *eps = a;
    temp = tempa + beta * (one + *eps);
    if *ibeta != 2 && *irnd != 0 {
        a = (a * (one + a)) / two;
        temp = one + a;
        if temp - one != zero {
            *eps = a;
        }
    }

    /* determine ngrd */

    *ngrd = 0;
    temp = one + *eps;
    if *irnd == 0 && temp * one - one != zero {
        *ngrd = 1;
    }

    /* determine iexp, minexp, xmin */

    /* loop to determine largest i and k = 2**i such that */
    /*        (1/beta) ** (2**(i)) */
    /* does not underflow. */
    /* exit from loop is signaled by an underflow. */

    i = 0;
    k = 1;
    z = betain;
    t = one + *eps;
    nxres = 0;
    loop {
        y = z;
        z = y * y;

        /* check for underflow here */

        a = z * one;
        temp = z * t;
        if a + a == zero || z.abs() >= y {
            break;
        }
        temp1 = temp * betain;
        if temp1 * beta == z {
            break;
        }
        i += 1;
        k = k + k;
    }
    if *ibeta != 10 {
        *iexp = i + 1;
        mx = k + k;
    } else {
        /* this segment is for decimal machines only */

        *iexp = 2;
        iz = *ibeta;
        while k >= iz {
            iz *= *ibeta;
            *iexp += 1;
        }
        mx = iz + iz - 1;
    }

    /* do { ... } while(temp1 * beta != y); with goto L10 inside */
    loop {
        /* loop to determine minexp, xmin */
        /* exit from loop is signaled by an underflow */

        *xmin = y;
        y *= betain;

        /* check for underflow here */

        a = y * one;
        temp = y * t;
        if a + a == zero || y.abs() >= *xmin {
            break; /* goto L10 */
        }
        k += 1;
        temp1 = temp * betain;
        if temp1 * beta == y {
            /* while condition false -> exit loop, fall through */
            break;
        }
        /* while condition true -> continue loop */
    }
    if temp1 * beta == y {
        /* exited because while condition was false, not because of goto L10 */
        nxres = 3;
        *xmin = y;
    }
    /* L10: */
    *minexp = -k;

    /* determine maxexp, xmax */

    if mx <= k + k - 3 && *ibeta != 10 {
        mx = mx + mx;
        *iexp += 1;
    }
    *maxexp = mx + *minexp;

    /* adjust irnd to reflect partial underflow */

    *irnd += nxres;

    /* adjust for ieee-style machines */

    if *irnd == 2 || *irnd == 5 {
        *maxexp -= 2;
    }

    /* adjust for non-ieee machines with partial underflow */

    if *irnd == 3 || *irnd == 4 {
        *maxexp -= *it;
    }

    /* adjust for machines with implicit leading bit in binary */
    /* significand, and machines with radix point at extreme */
    /* right of significand. */

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
    *xmax /= beta * beta * beta * *xmin;
    i = *maxexp + *minexp + 3;
    if i > 0 {
        j = 1;
        while j <= i {
            if *ibeta == 2 {
                *xmax = *xmax + *xmax;
            }
            if *ibeta != 2 {
                *xmax *= beta;
            }
            j += 1;
        }
    }
}
