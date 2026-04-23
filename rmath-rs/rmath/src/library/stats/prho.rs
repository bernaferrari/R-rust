//! Spearman's rho probability (AS 89)
//! Port of r-source/src/library/stats/src/prho.c

use std::os::raw::{c_double, c_int};

use crate::main::coerce::{asInteger, asReal};
use crate::nmath::dist::normal::pnorm5_inner;
use crate::sexp::constructors::Rf_ScalarReal;
use crate::sexp::ffi::{SEXP, SEXPTYPE};

unsafe fn prho(n: c_int, is: c_double, pv: *mut c_double, ifault: *mut c_int, lower_tail: c_int) {
    // Edgeworth coefficients
    let c1: c_double = 0.2274;
    let c2: c_double = 0.2531;
    let c3: c_double = 0.1745;
    let c4: c_double = 0.0758;
    let c5: c_double = 0.1033;
    let c6: c_double = 0.3932;
    let c7: c_double = 0.0879;
    let c8: c_double = 0.0151;
    let c9: c_double = 0.0072;
    let c10: c_double = 0.0831;
    let c11: c_double = 0.0131;
    let c12: c_double = 4.6e-4;

    let n_small: c_int = 9;
    let mut l = vec![0i32; n_small as usize];

    // Test admissibility of arguments and initialize
    *pv = if lower_tail != 0 { 0.0 } else { 1.0 };
    if n <= 1 {
        *ifault = 1;
        return;
    }

    *ifault = 0;
    if is <= 0.0 {
        return; // with p = 1
    }

    let mut n3 = n as c_double;
    n3 *= (n3 * n3 - 1.0) / 3.0;
    if is > n3 {
        // larger than maximal value
        *pv = 1.0 - *pv;
        return;
    }

    if n <= n_small {
        // 2 <= n <= n_small: Exact evaluation of probability
        let mut nfac: i64 = 1;
        let mut i: c_int = 1;
        while i <= n {
            nfac *= i as i64;
            l[(i - 1) as usize] = i;
            i += 1;
        }
        let mut ifr: i64;
        if is == n3 {
            ifr = 1;
        } else {
            ifr = 0;
            let mut m: i64 = 0;
            while m < nfac {
                let mut ise: i64 = 0;
                let mut i: c_int = 0;
                while i < n {
                    let n1 = (i + 1 - l[i as usize]) as i64;
                    ise += n1 * n1;
                    i += 1;
                }
                if is <= ise as c_double {
                    ifr += 1;
                }

                let mut n1 = n;
                loop {
                    let mt = l[0];
                    let mut i = 1;
                    while i < n1 {
                        l[(i - 1) as usize] = l[i as usize];
                        i += 1;
                    }
                    n1 -= 1;
                    l[n1 as usize] = mt;
                    if !(mt == n1 + 1 && n1 > 1) {
                        break;
                    }
                }
                m += 1;
            }
        }
        *pv = if lower_tail != 0 {
            (nfac - ifr) as c_double
        } else {
            ifr as c_double
        } / nfac as c_double;
    } else {
        // n >= 10: Evaluation by Edgeworth series expansion
        let y = n as c_double;
        let b = 1.0 / y;
        let x = (6.0 * (is - 1.0) * b / (y * y - 1.0) - 1.0) * (y - 1.0).sqrt();
        let y = x * x;
        let u = x
            * b
            * (c1
                + b * (c2 + c3 * b)
                + y * (-c4 + b * (c5 + c6 * b)
                    - y * b * (c7 + c8 * b - y * (c9 - c10 * b + y * b * (c11 - c12 * y)))));
        let y = u / (y / 2.0).exp();
        *pv = (if lower_tail != 0 { -y } else { y })
            + pnorm5_inner(x, 0.0, 1.0, lower_tail != 0, false);
        if *pv < 0.0 {
            *pv = 0.0;
        }
        if *pv > 1.0 {
            *pv = 1.0;
        }
    }
}

pub unsafe fn pRho(q: SEXP, sn: SEXP, lower: SEXP) -> SEXP {
    let s = asReal(q);
    let mut p: c_double = 0.0;
    let n = asInteger(sn);
    let ltail = asInteger(lower);
    let mut ifault: c_int = 0;
    prho(n, s, &mut p, &mut ifault, ltail);
    if ifault != 0 {
        eprintln!("invalid sample size 'n' in C routine prho(n,s,*)");
    }
    Rf_ScalarReal(p)
}
