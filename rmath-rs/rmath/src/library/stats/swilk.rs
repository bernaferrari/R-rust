#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

//! Shapiro-Wilk W test
//! Port of r-source/src/library/stats/src/swilk.c
//! Based on Applied Statistics algorithms AS181, R94

use std::os::raw::{c_double, c_int};

use crate::main::coerce::coerceVector;
use crate::nmath::dist::normal::pnorm5_inner;
use crate::nmath::dist::normal::qnorm5_inner;
use crate::sexp::accessors::{LENGTH, REAL, TYPEOF};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

unsafe fn poly(cc: *const c_double, nord: c_int, x: c_double) -> c_double {
    let mut ret_val = *cc;
    if nord > 1 {
        let mut p = x * *cc.add((nord - 1) as usize);
        let mut j = nord - 2;
        while j > 0 {
            p = (p + *cc.add(j as usize)) * x;
            j -= 1;
        }
        ret_val += p;
    }
    ret_val
}

unsafe fn swilk(
    x: *const c_double,
    n: c_int,
    w: *mut c_double,
    pw: *mut c_double,
    ifault: *mut c_int,
) {
    let nn2 = n / 2;
    let mut a = vec![0.0f64; (nn2 + 1) as usize]; // 1-based

    let small = 1e-19;

    // polynomial coefficients
    let g: [c_double; 2] = [-2.273, 0.459];
    let c1: [c_double; 6] = [0.0, 0.221157, -0.147981, -2.07119, 4.434685, -2.706056];
    let c2: [c_double; 6] = [0.0, 0.042981, -0.293762, -1.752461, 5.682633, -3.582633];
    let c3: [c_double; 4] = [0.544, -0.39978, 0.025054, -6.714e-4];
    let c4: [c_double; 4] = [1.3822, -0.77857, 0.062767, -0.0020322];
    let c5: [c_double; 4] = [-1.5861, -0.31082, -0.083751, 0.0038915];
    let c6: [c_double; 3] = [-0.4803, -0.082676, 0.0030302];

    let an = n as c_double;

    *pw = 1.0;
    if n < 3 {
        *ifault = 1;
        return;
    }

    if n == 3 {
        a[1] = 0.70710678; // sqrt(1/2)
    } else {
        let an25 = an + 0.25;
        let mut summ2 = 0.0;
        let mut i: c_int = 1;
        while i <= nn2 {
            a[i as usize] = qnorm5_inner((i as c_double - 0.375) / an25, 0.0, 1.0, true, false);
            let r1 = a[i as usize];
            summ2 += r1 * r1;
            i += 1;
        }
        summ2 *= 2.0;
        let ssumm2 = summ2.sqrt();
        let rsn = 1.0 / an.sqrt();
        let mut a1 = poly(c1.as_ptr(), 6, rsn) - a[1] / ssumm2;

        let mut i1: c_int;
        let fac: c_double;
        if n > 5 {
            i1 = 3;
            let a2 = -a[2] / ssumm2 + poly(c2.as_ptr(), 6, rsn);
            fac = ((summ2 - 2.0 * (a[1] * a[1]) - 2.0 * (a[2] * a[2]))
                / (1.0 - 2.0 * (a1 * a1) - 2.0 * (a2 * a2)))
                .sqrt();
            a[2] = a2;
        } else {
            i1 = 2;
            fac = ((summ2 - 2.0 * (a[1] * a[1])) / (1.0 - 2.0 * (a1 * a1))).sqrt();
        }
        a[1] = a1;
        let mut i = i1;
        while i <= nn2 {
            a[i as usize] /= -fac;
            i += 1;
        }
    }

    // Check for zero range
    let range = *x.add((n - 1) as usize) - *x.add(0);
    if range < small {
        *ifault = 6;
        return;
    }

    // Check for correct sort order on range-scaled X
    *ifault = 0;
    let mut xx = *x.add(0) / range;
    let mut sx = xx;
    let mut sa = -a[1];
    let mut i: c_int = 1;
    let mut j = n - 1;
    while i < n {
        let xi = *x.add(i as usize) / range;
        if xx - xi > small {
            *ifault = 7;
        }
        sx += xi;
        i += 1;
        if i != j {
            let m = if i < j { i } else { j };
            sa += (i - j).signum() as c_double * a[m as usize];
        }
        xx = xi;
        j -= 1;
    }
    if n > 5000 {
        *ifault = 2;
    }

    // Calculate W statistic
    sa /= n as c_double;
    sx /= n as c_double;
    let mut ssa = 0.0;
    let mut ssx = 0.0;
    let mut sax = 0.0;
    let mut i: c_int = 0;
    let mut j = n - 1;
    while i < n {
        let asa = if i != j {
            let m = if i < j { i } else { j };
            (i - j).signum() as c_double * a[(1 + m) as usize] - sa
        } else {
            -sa
        };
        let xsx = *x.add(i as usize) / range - sx;
        ssa += asa * asa;
        ssx += xsx * xsx;
        sax += asa * xsx;
        i += 1;
        j -= 1;
    }

    let ssassx = (ssa * ssx).sqrt();
    let w1 = (ssassx - sax) * (ssassx + sax) / (ssa * ssx);
    *w = 1.0 - w1;

    // Calculate significance level for W
    if n == 3 {
        let pi6 = 1.90985931710274; // 6/pi
        let stqr = 1.04719755119660; // asin(sqrt(3/4))
        *pw = pi6 * ((*w).sqrt().asin() - stqr);
        if *pw < 0.0 {
            *pw = 0.0;
        }
        return;
    }

    let y = w1.ln();
    let xx = an.ln();
    let (m, s) = if n <= 11 {
        let gamma = poly(g.as_ptr(), 2, an);
        if y >= gamma {
            *pw = 1e-99;
            return;
        }
        let y = -(gamma - y).ln();
        let m = poly(c3.as_ptr(), 4, an);
        let s = poly(c4.as_ptr(), 4, an).exp();
        (m, s)
    } else {
        let m = poly(c5.as_ptr(), 4, xx);
        let s = poly(c6.as_ptr(), 3, xx).exp();
        (m, s)
    };

    *pw = pnorm5_inner(y, m, s, false, false); // upper tail
}

pub unsafe fn SWilk(x: SEXP) -> SEXP {
    let mut ifault: c_int = 0;
    let mut W: c_double = 0.0;
    let mut pw: c_double = 0.0;

    let x = Rf_protect(coerceVector(x, SEXPTYPE::REALSXP.0));
    let n = LENGTH(x);
    swilk(REAL(x), n, &mut W, &mut pw, &mut ifault);
    if ifault > 0 && ifault != 7 {
        eprintln!("ifault={}. This should not happen", ifault);
    }
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, 2));
    *REAL(ans) = W;
    *REAL(ans).add(1) = pw;
    Rf_unprotect(2);
    ans
}
