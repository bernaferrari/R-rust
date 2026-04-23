#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]

//! Lowess (locally weighted scatterplot smoothing)
//! Port of r-source/src/library/stats/src/lowess.c

use std::os::raw::{c_double, c_int};

use crate::main::coerce::{asInteger, asReal};
use crate::main::errors::Rf_error;
use crate::main::sort::rPsort;
use crate::sexp::accessors::{LENGTH, REAL, TYPEOF};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::NA_INTEGER;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

#[inline]
fn fsquare(x: c_double) -> c_double {
    x * x
}

#[inline]
fn fcube(x: c_double) -> c_double {
    x * x * x
}

#[inline]
fn fmax2(a: c_double, b: c_double) -> c_double {
    if a > b { a } else { b }
}

#[inline]
fn imin2(a: c_int, b: c_int) -> c_int {
    if a < b { a } else { b }
}

#[inline]
fn imax2(a: c_int, b: c_int) -> c_int {
    if a > b { a } else { b }
}

unsafe fn lowest(
    x: *mut c_double,
    y: *mut c_double,
    n: c_int,
    xs: *const c_double,
    ys: *mut c_double,
    nleft: c_int,
    nright: c_int,
    w: *mut c_double,
    userw: bool,
    rw: *const c_double,
    ok: *mut bool,
) {
    let mut nrt: c_int;
    let mut j: c_int;
    let mut a: c_double;
    let mut b: c_double;
    let mut c: c_double;
    let mut h: c_double;
    let mut h1: c_double;
    let mut h9: c_double;
    let mut r: c_double;
    let range: c_double;

    range = *x.add(n as usize) - *x.add(1);
    h = fmax2(*xs - *x.add(nleft as usize), *x.add(nright as usize) - *xs);
    h9 = 0.999 * h;
    h1 = 0.001 * h;

    // sum of weights
    a = 0.0;
    j = nleft;
    while j <= n {
        *w.add(j as usize) = 0.0;
        r = (*x.add(j as usize) - *xs).abs();
        if r <= h9 {
            if r <= h1 {
                *w.add(j as usize) = 1.0;
            } else {
                *w.add(j as usize) = fcube(1.0 - fcube(r / h));
            }
            if userw {
                *w.add(j as usize) *= *rw.add(j as usize);
            }
            a += *w.add(j as usize);
        } else if *x.add(j as usize) > *xs {
            break;
        }
        j += 1;
    }

    nrt = j - 1;
    if a <= 0.0 {
        *ok = false;
    } else {
        *ok = true;

        // weighted least squares
        // make sum of w[j] == 1
        j = nleft;
        while j <= nrt {
            *w.add(j as usize) /= a;
            j += 1;
        }
        if h > 0.0 {
            a = 0.0;

            // use linear fit
            // weighted center of x values
            j = nleft;
            while j <= nrt {
                a += *w.add(j as usize) * *x.add(j as usize);
                j += 1;
            }
            b = *xs - a;
            c = 0.0;
            j = nleft;
            while j <= nrt {
                c += *w.add(j as usize) * fsquare(*x.add(j as usize) - a);
                j += 1;
            }
            if c.sqrt() > 0.001 * range {
                b /= c;

                // points are spread out enough to compute slope
                j = nleft;
                while j <= nrt {
                    *w.add(j as usize) *= b * (*x.add(j as usize) - a) + 1.0;
                    j += 1;
                }
            }
        }
        *ys = 0.0;
        j = nleft;
        while j <= nrt {
            *ys += *w.add(j as usize) * *y.add(j as usize);
            j += 1;
        }
    }
}

unsafe fn clowess(
    x: *const c_double,
    y: *const c_double,
    n: c_int,
    f: c_double,
    nsteps: c_int,
    delta: c_double,
    ys: *mut c_double,
    rw: *mut c_double,
    res: *mut c_double,
) {
    if n < 2 {
        *ys = *y;
        return;
    }

    // at least two, at most n points
    let ns = imax2(2, imin2(n, (f * n as c_double + 1e-7) as c_int));

    let mut iter: c_int = 1;
    while iter <= nsteps + 1 {
        let mut nleft: c_int = 1;
        let mut nright: c_int = ns;
        let mut last: c_int = 0;
        let mut i: c_int = 1;

        loop {
            if nright < n {
                let d1 = *x.add((i - 1) as usize) - *x.add((nleft - 1) as usize);
                let d2 = *x.add(nright as usize) - *x.add((i - 1) as usize);

                if d1 > d2 {
                    nleft += 1;
                    nright += 1;
                    continue;
                }
            }

            // fitted value at x[i] (1-based)
            let mut ok = false;
            lowest(
                x as *mut c_double,
                y as *mut c_double,
                n,
                &*x.add((i - 1) as usize),
                ys.add(i as usize),
                nleft,
                nright,
                res,
                iter > 1,
                rw,
                &mut ok,
            );
            if !ok {
                *ys.add(i as usize) = *y.add((i - 1) as usize);
            }

            // skipped points -- interpolate
            if last < i - 1 {
                let denom = *x.add((i - 1) as usize) - *x.add((last - 1) as usize);
                let mut j = last + 1;
                while j < i {
                    let alpha = (*x.add((j - 1) as usize) - *x.add((last - 1) as usize)) / denom;
                    *ys.add(j as usize) =
                        alpha * *ys.add(i as usize) + (1.0 - alpha) * *ys.add(last as usize);
                    j += 1;
                }
            }

            last = i;

            // x coord of close points
            let cut = *x.add((last - 1) as usize) + delta;
            i = last + 1;
            while i <= n {
                if *x.add((i - 1) as usize) > cut {
                    break;
                }
                if *x.add((i - 1) as usize) == *x.add((last - 1) as usize) {
                    *ys.add(i as usize) = *ys.add(last as usize);
                    last = i;
                }
                i += 1;
            }
            i = imax2(last + 1, i - 1);
            if last >= n {
                break;
            }
        }

        // residuals
        let mut i: c_int = 0;
        while i < n {
            *res.add(i as usize) = *y.add(i as usize) - *ys.add((i + 1) as usize);
            i += 1;
        }

        // overall scale estimate
        let mut sc: c_double = 0.0;
        let mut i: c_int = 0;
        while i < n {
            sc += (*res.add(i as usize)).abs();
            i += 1;
        }
        sc /= n as c_double;

        // compute robustness weights except last time
        if iter > nsteps {
            break;
        }

        let mut i: c_int = 0;
        while i < n {
            *rw.add(i as usize) = (*res.add(i as usize)).abs();
            i += 1;
        }

        // Compute cmad := 6 * median(rw[], n)
        let m1 = n / 2;
        rPsort(rw, n, m1);
        let cmad = if n % 2 == 0 {
            let m2 = n - m1 - 1;
            rPsort(rw, n, m2);
            3.0 * (*rw.add(m1 as usize) + *rw.add(m2 as usize))
        } else {
            6.0 * *rw.add(m1 as usize)
        };

        if cmad < 1e-7 * sc {
            break;
        }
        let c9 = 0.999 * cmad;
        let c1 = 0.001 * cmad;
        let mut i: c_int = 0;
        while i < n {
            let r = (*res.add(i as usize)).abs();
            if r <= c1 {
                *rw.add(i as usize) = 1.0;
            } else if r <= c9 {
                *rw.add(i as usize) = fsquare(1.0 - fsquare(r / cmad));
            } else {
                *rw.add(i as usize) = 0.0;
            }
            i += 1;
        }
        iter += 1;
    }
}

pub unsafe fn lowess(x: SEXP, y: SEXP, sf: SEXP, siter: SEXP, sdelta: SEXP) -> SEXP {
    if TYPEOF(x) != SEXPTYPE::REALSXP || TYPEOF(y) != SEXPTYPE::REALSXP {
        Rf_error(b"invalid input\0".as_ptr() as *const _);
    }
    let nx = LENGTH(x);
    if nx == NA_INTEGER || nx == 0 {
        Rf_error(b"invalid input\0".as_ptr() as *const _);
    }
    let f = asReal(sf);
    if !f.is_finite() || f <= 0.0 {
        Rf_error(b"'f' must be finite and > 0\0".as_ptr() as *const _);
    }
    let iter = asInteger(siter);
    if iter == NA_INTEGER || iter < 0 {
        Rf_error(b"'iter' must be finite and >= 0\0".as_ptr() as *const _);
    }
    let delta = asReal(sdelta);
    if !delta.is_finite() || delta < 0.0 {
        Rf_error(b"'delta' must be finite and > 0\0".as_ptr() as *const _);
    }

    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, nx));
    let mut rw = vec![0.0f64; nx as usize];
    let mut res = vec![0.0f64; nx as usize];
    clowess(
        REAL(x),
        REAL(y),
        nx,
        f,
        iter,
        delta,
        REAL(ans),
        rw.as_mut_ptr(),
        res.as_mut_ptr(),
    );
    Rf_unprotect(1);
    ans
}
