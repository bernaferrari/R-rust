#![allow(unsafe_op_in_unsafe_fn)] // legacy C-port unsafe boundary; see docs/unsafe-op-allowlist.tsv.
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/stats/src/splines.c
 *
 *  Spline Interpolation — natural, periodic, and FMM splines.
 */

use std::os::raw::{c_char, c_double, c_int};

use crate::attrib_core::{R_NamesSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, coerceVector};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

unsafe fn getListElement(list: SEXP, str: *const c_char) -> SEXP {
    if TYPEOF(list) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }
    let names = getAttrib(list, R_NamesSymbol());
    let len = LENGTH(list);
    let c_str = std::ffi::CStr::from_ptr(str);
    let target = c_str.to_bytes();
    let mut i = 0;
    while i < len {
        if Rf_isNull(STRING_ELT(names, i as R_xlen_t)) == 0 {
            let nm = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i as R_xlen_t)));
            if nm.to_bytes() == target {
                return VECTOR_ELT(list, i as R_xlen_t);
            }
        }
        i += 1;
    }
    R_NilValue()
}

unsafe fn asXlen(x: SEXP) -> R_xlen_t {
    let t = TYPEOF(x);
    if (t >= SEXPTYPE::INTSXP.into() && t <= SEXPTYPE::REALSXP.into()) && XLENGTH(x) >= 1 {
        if t == SEXPTYPE::INTSXP {
            INTEGER(x).add(0).read() as R_xlen_t
        } else if t == SEXPTYPE::REALSXP {
            REAL(x).add(0).read() as R_xlen_t
        } else {
            NA_INTEGER as R_xlen_t
        }
    } else {
        NA_INTEGER as R_xlen_t
    }
}

// ---------------------------------------------------------------------------
// Spline algorithms (using usize for internal indexing)
// ---------------------------------------------------------------------------

/// Natural spline — end-conditions: second derivative = 0 at endpoints.
unsafe fn natural_spline(
    n: usize,
    x: *const c_double,
    y: *const c_double,
    b: *mut c_double,
    c: *mut c_double,
    d: *mut c_double,
) {
    if n < 2 {
        return;
    }
    if n < 3 {
        let t = *y.add(1) - *y.add(0);
        *b.add(0) = t / (*x.add(1) - *x.add(0));
        *b.add(1) = *b.add(0);
        *c.add(0) = 0.0;
        *c.add(1) = 0.0;
        *d.add(0) = 0.0;
        *d.add(1) = 0.0;
        return;
    }

    let nm1 = n - 1;

    // Set up tridiagonal system
    *d.add(0) = *x.add(1) - *x.add(0);
    *c.add(1) = (*y.add(1) - *y.add(0)) / *d.add(0);
    let mut i = 1usize;
    while i < nm1 {
        *d.add(i) = *x.add(i + 1) - *x.add(i);
        *b.add(i) = 2.0 * (*d.add(i - 1) + *d.add(i));
        *c.add(i + 1) = (*y.add(i + 1) - *y.add(i)) / *d.add(i);
        *c.add(i) = *c.add(i + 1) - *c.add(i);
        i += 1;
    }

    // Gaussian elimination
    i = 2;
    while i < nm1 {
        let t = *d.add(i - 1) / *b.add(i - 1);
        *b.add(i) = *b.add(i) - t * *d.add(i - 1);
        *c.add(i) = *c.add(i) - t * *c.add(i - 1);
        i += 1;
    }

    // Backward substitution
    *c.add(nm1 - 1) = *c.add(nm1 - 1) / *b.add(nm1 - 1);
    i = nm1 - 2;
    while i >= 1 {
        *c.add(i) = (*c.add(i) - *d.add(i) * *c.add(i + 1)) / *b.add(i);
        i -= 1;
    }

    // End conditions
    *c.add(0) = 0.0;
    *c.add(nm1) = 0.0;

    // Cubic coefficients
    *b.add(0) = (*y.add(1) - *y.add(0)) / *d.add(0) - *d.add(nm1 - 1) * *c.add(1);
    *c.add(0) = 0.0;
    *d.add(0) = *c.add(1) / *d.add(0);
    *b.add(nm1) =
        (*y.add(nm1) - *y.add(nm1 - 1)) / *d.add(nm1 - 1) + *d.add(nm1 - 1) * *c.add(nm1 - 1);
    i = 1;
    while i < nm1 {
        *b.add(i) =
            (*y.add(i + 1) - *y.add(i)) / *d.add(i) - *d.add(i) * (*c.add(i + 1) + 2.0 * *c.add(i));
        *d.add(i) = (*c.add(i + 1) - *c.add(i)) / *d.add(i);
        *c.add(i) = 3.0 * *c.add(i);
        i += 1;
    }
    *c.add(nm1) = 0.0;
    *d.add(nm1) = 0.0;
}

/// FMM spline — end-conditions from Forsythe, Malcolm, and Moler.
unsafe fn fmm_spline(
    n: usize,
    x: *const c_double,
    y: *const c_double,
    b: *mut c_double,
    c: *mut c_double,
    d: *mut c_double,
) {
    if n < 2 {
        return;
    }
    if n < 3 {
        let t = *y.add(1) - *y.add(0);
        *b.add(0) = t / (*x.add(1) - *x.add(0));
        *b.add(1) = *b.add(0);
        *c.add(0) = 0.0;
        *c.add(1) = 0.0;
        *d.add(0) = 0.0;
        *d.add(1) = 0.0;
        return;
    }

    let nm1 = n - 1;

    // Set up tridiagonal system
    *d.add(0) = *x.add(1) - *x.add(0);
    *c.add(1) = (*y.add(1) - *y.add(0)) / *d.add(0);
    let mut i = 1usize;
    while i < nm1 {
        *d.add(i) = *x.add(i + 1) - *x.add(i);
        *b.add(i) = 2.0 * (*d.add(i - 1) + *d.add(i));
        *c.add(i + 1) = (*y.add(i + 1) - *y.add(i)) / *d.add(i);
        *c.add(i) = *c.add(i + 1) - *c.add(i);
        i += 1;
    }

    // End conditions
    *b.add(0) = -*d.add(0);
    *b.add(nm1) = -*d.add(nm1 - 1);
    *c.add(0) = 0.0;
    *c.add(nm1) = 0.0;
    if n > 3 {
        *c.add(0) = *c.add(2) / (*x.add(3) - *x.add(1)) - *c.add(1) / (*x.add(2) - *x.add(0));
        *c.add(nm1) = *c.add(nm1 - 1) / (*x.add(nm1) - *x.add(nm1 - 2))
            - *c.add(nm1 - 2) / (*x.add(nm1 - 1) - *x.add(nm1 - 3));
        *c.add(0) = *c.add(0) * *d.add(0) * *d.add(0) / (*x.add(3) - *x.add(0));
        *c.add(nm1) =
            -*c.add(nm1) * *d.add(nm1 - 1) * *d.add(nm1 - 1) / (*x.add(nm1) - *x.add(nm1 - 3));
    }

    // Gaussian elimination
    i = 1;
    while i < nm1 {
        let t = *d.add(i - 1) / *b.add(i - 1);
        *b.add(i) = *b.add(i) - t * *d.add(i - 1);
        *c.add(i) = *c.add(i) - t * *c.add(i - 1);
        i += 1;
    }

    // Backward substitution
    *c.add(nm1) = *c.add(nm1) / *b.add(nm1);
    i = nm1 - 1;
    while i >= 1 {
        i -= 1;
    }
    i = nm1 - 1;
    while i >= 1 {
        *c.add(i) = (*c.add(i) - *d.add(i) * *c.add(i + 1)) / *b.add(i);
        if i == 0 {
            break;
        }
        i -= 1;
    }

    // Polynomial coefficients
    *b.add(nm1) = (*y.add(nm1) - *y.add(nm1 - 1)) / *d.add(nm1 - 1)
        + *d.add(nm1 - 1) * (*c.add(nm1 - 1) + 2.0 * *c.add(nm1));
    i = 0;
    while i < nm1 {
        *b.add(i) =
            (*y.add(i + 1) - *y.add(i)) / *d.add(i) - *d.add(i) * (*c.add(i + 1) + 2.0 * *c.add(i));
        *d.add(i) = (*c.add(i + 1) - *c.add(i)) / *d.add(i);
        *c.add(i) = 3.0 * *c.add(i);
        i += 1;
    }
    *c.add(nm1) = 3.0 * *c.add(nm1);
    *d.add(nm1) = *d.add(nm1 - 1);
}

/// Periodic spline — end-conditions match spline at x[1] and x[n].
unsafe fn periodic_spline(
    n: usize,
    x: *const c_double,
    y: *const c_double,
    b: *mut c_double,
    c: *mut c_double,
    d: *mut c_double,
) {
    if n < 2 || *y.add(0) != *y.add(n - 1) {
        return;
    }

    if n == 2 {
        *b.add(0) = 0.0;
        *b.add(1) = 0.0;
        *c.add(0) = 0.0;
        *c.add(1) = 0.0;
        *d.add(0) = 0.0;
        *d.add(1) = 0.0;
        return;
    }

    let nm1 = n - 1;

    if n == 3 {
        let val = -(*y.add(0) - *y.add(1)) * (*x.add(0) - 2.0 * *x.add(1) + *x.add(2))
            / (*x.add(2) - *x.add(1))
            / (*x.add(1) - *x.add(0));
        *b.add(0) = val;
        *b.add(1) = val;
        *b.add(2) = val;
        let cv = -3.0 * (*y.add(0) - *y.add(1)) / (*x.add(2) - *x.add(1)) / (*x.add(1) - *x.add(0));
        *c.add(0) = cv;
        *c.add(1) = -cv;
        *c.add(2) = cv;
        let dv = -2.0 * cv / 3.0 / (*x.add(1) - *x.add(0));
        *d.add(0) = dv;
        *d.add(1) = -dv * (*x.add(1) - *x.add(0)) / (*x.add(2) - *x.add(1));
        *d.add(2) = dv;
        return;
    }

    // n >= 4
    let mut e: Vec<c_double> = vec![0.0; n];
    let mut s: c_double;
    let mut i: usize;

    // Set up matrix system: A=b, B=d, C=c
    *d.add(0) = *x.add(1) - *x.add(0);
    *d.add(nm1 - 1) = *x.add(nm1) - *x.add(nm1 - 1);
    *b.add(0) = 2.0 * (*d.add(0) + *d.add(nm1 - 1));
    *c.add(0) =
        (*y.add(1) - *y.add(0)) / *d.add(0) - (*y.add(nm1) - *y.add(nm1 - 1)) / *d.add(nm1 - 1);

    i = 1;
    while i < nm1 {
        *d.add(i) = *x.add(i + 1) - *x.add(i);
        *b.add(i) = 2.0 * (*d.add(i) + *d.add(i - 1));
        *c.add(i) =
            (*y.add(i + 1) - *y.add(i)) / *d.add(i) - (*y.add(i) - *y.add(i - 1)) / *d.add(i - 1);
        i += 1;
    }

    // Cholesky: L=b, M=d, E=e
    *b.add(0) = (*b.add(0)).sqrt();
    e[0] = (*x.add(nm1) - *x.add(nm1 - 1)) / *b.add(0);
    s = 0.0;
    i = 0;
    while i <= nm1 - 3 {
        *d.add(i) = *d.add(i) / *b.add(i);
        if i != 0 {
            e[i] = -e[i - 1] * *d.add(i - 1) / *b.add(i);
        }
        *b.add(i + 1) = (*b.add(i + 1) - *d.add(i) * *d.add(i)).sqrt();
        s += e[i] * e[i];
        i += 1;
    }
    *d.add(nm1 - 2) = (*d.add(nm1 - 2) - e[nm1 - 3] * *d.add(nm1 - 3)) / *b.add(nm1 - 2);
    *b.add(nm1 - 1) = (*b.add(nm1 - 1) - *d.add(nm1 - 2) * *d.add(nm1 - 2) - s).sqrt();

    // Forward elimination: Y=c
    *c.add(0) = *c.add(0) / *b.add(0);
    s = 0.0;
    i = 1;
    while i <= nm1 - 2 {
        *c.add(i) = (*c.add(i) - *d.add(i - 1) * *c.add(i - 1)) / *b.add(i);
        s += e[i - 1] * *c.add(i - 1);
        i += 1;
    }
    *c.add(nm1 - 1) = (*c.add(nm1 - 1) - *d.add(nm1 - 2) * *c.add(nm1 - 2) - s) / *b.add(nm1 - 1);

    // Back substitution
    *c.add(nm1 - 1) = *c.add(nm1 - 1) / *b.add(nm1 - 1);
    *c.add(nm1 - 2) = (*c.add(nm1 - 2) - *d.add(nm1 - 2) * *c.add(nm1 - 1)) / *b.add(nm1 - 2);
    i = nm1 - 3;
    while i >= 1 {
        *c.add(i) = (*c.add(i) - *d.add(i) * *c.add(i + 1) - e[i] * *c.add(nm1 - 1)) / *b.add(i);
        i -= 1;
    }
    if nm1 >= 3 {
        i = nm1 - 3;
        // already handled in the loop above (goes down to 1)
    }

    // Wrap around
    *c.add(nm1) = *c.add(0);

    // Polynomial coefficients
    i = 0;
    while i < nm1 {
        s = *x.add(i + 1) - *x.add(i);
        *b.add(i) = (*y.add(i + 1) - *y.add(i)) / s - s * (*c.add(i + 1) + 2.0 * *c.add(i));
        *d.add(i) = (*c.add(i + 1) - *c.add(i)) / s;
        *c.add(i) = 3.0 * *c.add(i);
        i += 1;
    }
    *b.add(nm1) = *b.add(0);
    *c.add(nm1) = *c.add(0);
    *d.add(nm1) = *d.add(0);
}

unsafe fn spline_coef(
    method: c_int,
    n: usize,
    x: *mut c_double,
    y: *mut c_double,
    b: *mut c_double,
    c: *mut c_double,
    d: *mut c_double,
) {
    match method {
        1 => periodic_spline(n, x, y, b, c, d),
        2 => natural_spline(n, x, y, b, c, d),
        3 => fmm_spline(n, x, y, b, c, d),
        _ => {} // intentionally unhandled: unknown spline method
    }
}

unsafe fn spline_eval(
    method: c_int,
    nu: usize,
    u: *const c_double,
    v: *mut c_double,
    n: usize,
    x: *const c_double,
    y: *const c_double,
    b: *const c_double,
    c: *const c_double,
    d: *const c_double,
) {
    let n_1 = n - 1;
    let mut i: usize = 0;

    if method == 1 && n > 1 {
        let dx = *x.add(n_1) - *x.add(0);
        let mut l = 0usize;
        while l < nu {
            *v.add(l) = (*u.add(l) - *x.add(0)) % dx;
            if *v.add(l) < 0.0 {
                *v.add(l) += dx;
            }
            *v.add(l) += *x.add(0);
            l += 1;
        }
    } else {
        let mut l = 0usize;
        while l < nu {
            *v.add(l) = *u.add(l);
            l += 1;
        }
    }

    let mut l = 0usize;
    while l < nu {
        let ul = *v.add(l);
        if ul < *x.add(i) || (i < n_1 && *x.add(i + 1) < ul) {
            i = 0;
            let mut j = n;
            loop {
                let k = (i + j) / 2;
                if ul < *x.add(k) {
                    j = k;
                } else {
                    i = k;
                }
                if j <= i + 1 {
                    break;
                }
            }
        }
        let dx = ul - *x.add(i);
        let tmp = if method == 2 && ul < *x.add(0) {
            0.0
        } else {
            *d.add(i)
        };
        *v.add(l) = *y.add(i) + dx * (*b.add(i) + dx * (*c.add(i) + dx * tmp));
        l += 1;
    }
}

// ---------------------------------------------------------------------------
// Exported SEXP functions
// ---------------------------------------------------------------------------

/// SplineCoef - compute spline coefficients.
pub unsafe fn SplineCoef(method: SEXP, x: SEXP, y: SEXP) -> SEXP {
    let x = Rf_protect(coerceVector(x, SEXPTYPE::REALSXP.into()));
    let y = Rf_protect(coerceVector(y, SEXPTYPE::REALSXP.into()));
    let n = XLENGTH(x) as usize;
    let m = asInteger(method);
    if XLENGTH(y) as usize != n {
        Rf_error(b"inputs of different lengths\0".as_ptr() as *const c_char);
    }

    let b = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, n as c_int));
    let c = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, n as c_int));
    let d = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, n as c_int));

    for i in 0..n {
        *REAL(b).add(i) = 0.0;
        *REAL(c).add(i) = 0.0;
        *REAL(d).add(i) = 0.0;
    }

    spline_coef(
        m,
        n,
        REAL(x) as *mut c_double,
        REAL(y) as *mut c_double,
        REAL(b) as *mut c_double,
        REAL(c) as *mut c_double,
        REAL(d) as *mut c_double,
    );

    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 7));
    SET_VECTOR_ELT(ans, 0, Rf_ScalarInteger(m));
    if n > i32::MAX as usize {
        SET_VECTOR_ELT(ans, 1, Rf_ScalarReal(n as c_double));
    } else {
        SET_VECTOR_ELT(ans, 1, Rf_ScalarInteger(n as c_int));
    }
    SET_VECTOR_ELT(ans, 2, x);
    SET_VECTOR_ELT(ans, 3, y);
    SET_VECTOR_ELT(ans, 4, b);
    SET_VECTOR_ELT(ans, 5, c);
    SET_VECTOR_ELT(ans, 6, d);

    let nm = Rf_allocVector(SEXPTYPE::STRSXP, 7);
    SET_STRING_ELT(nm, 0, Rf_mkChar(b"method\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 1, Rf_mkChar(b"n\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 2, Rf_mkChar(b"x\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 3, Rf_mkChar(b"y\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 4, Rf_mkChar(b"b\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 5, Rf_mkChar(b"c\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 6, Rf_mkChar(b"d\0".as_ptr() as *const c_char));
    setAttrib(ans, R_NamesSymbol(), nm);
    Rf_unprotect(6);
    ans
}

/// SplineEval - evaluate a spline at given points.
pub unsafe fn SplineEval(xout: SEXP, z: SEXP) -> SEXP {
    let xout = Rf_protect(coerceVector(xout, SEXPTYPE::REALSXP.into()));
    let nu = XLENGTH(xout) as usize;
    let z_n = getListElement(z, b"n\0".as_ptr() as *const c_char);
    let nx = asXlen(z_n) as usize;
    let yout = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, nu as c_int));

    let method = asInteger(getListElement(z, b"method\0".as_ptr() as *const c_char));
    let x = getListElement(z, b"x\0".as_ptr() as *const c_char);
    let y = getListElement(z, b"y\0".as_ptr() as *const c_char);
    let b = getListElement(z, b"b\0".as_ptr() as *const c_char);
    let c = getListElement(z, b"c\0".as_ptr() as *const c_char);
    let d = getListElement(z, b"d\0".as_ptr() as *const c_char);

    spline_eval(
        method,
        nu,
        REAL(xout),
        REAL(yout) as *mut c_double,
        nx,
        REAL(x),
        REAL(y),
        REAL(b),
        REAL(c),
        REAL(d),
    );
    Rf_unprotect(2);
    yout
}
