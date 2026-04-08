#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

//! Linear and Step Function Interpolation
//! Port of r-source/src/library/stats/src/approx.c

use std::os::raw::{c_double, c_int};

use crate::main::coerce::R_FINITE;
use crate::main::coerce::{asInteger, asLogical, asReal, coerceVector};
use crate::sexp::accessors::{REAL, TYPEOF, XLENGTH};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

struct appr_meth {
    ylow: c_double,
    yhigh: c_double,
    f1: c_double,
    f2: c_double,
    kind: c_int,
    _na_rm: c_int,
}

unsafe fn approx1(
    v: c_double,
    x: *const c_double,
    y: *const c_double,
    n: R_xlen_t,
    meth: &appr_meth,
) -> c_double {
    if n == 0 {
        return f64::NAN;
    }

    let mut i: R_xlen_t = 0;
    let mut j: R_xlen_t = n - 1;

    if v < *x.add(i as usize) {
        return meth.ylow;
    }
    if v > *x.add(j as usize) {
        return meth.yhigh;
    }

    while i < j - 1 {
        let ij = (i + j) / 2;
        if v < *x.add(ij as usize) {
            j = ij;
        } else {
            i = ij;
        }
    }

    if v == *x.add(j as usize) {
        return *y.add(j as usize);
    }
    if v == *x.add(i as usize) {
        return *y.add(i as usize);
    }

    if meth.kind == 1 {
        *y.add(i as usize)
            + (*y.add(j as usize) - *y.add(i as usize))
                * ((v - *x.add(i as usize)) / (*x.add(j as usize) - *x.add(i as usize)))
    } else {
        (if meth.f1 != 0.0 {
            *y.add(i as usize) * meth.f1
        } else {
            0.0
        }) + (if meth.f2 != 0.0 {
            *y.add(j as usize) * meth.f2
        } else {
            0.0
        })
    }
}

unsafe fn R_approxtest(
    x: *const c_double,
    y: *const c_double,
    nxy: R_xlen_t,
    method: c_int,
    f: c_double,
    na_rm: c_int,
) {
    match method {
        1 => {}
        2 => {
            if !R_FINITE(f) || f < 0.0 || f > 1.0 {
                eprintln!("approx(): invalid f value");
            }
        }
        _ => {
            eprintln!("approx(): invalid interpolation method");
        }
    }
    if na_rm != 0 {
        let mut i: R_xlen_t = 0;
        while i < nxy {
            if x.add(i as usize).read().is_nan() || y.add(i as usize).read().is_nan() {
                eprintln!("approx(): attempted to interpolate NA values");
            }
            i += 1;
        }
    } else {
        let mut i: R_xlen_t = 0;
        while i < nxy {
            if x.add(i as usize).read().is_nan() {
                eprintln!("approx(x,y, .., na.rm=FALSE): NA values in x are not allowed");
            }
            i += 1;
        }
    }
}

unsafe fn R_approxfun(
    x: *const c_double,
    y: *const c_double,
    nxy: R_xlen_t,
    xout: *const c_double,
    yout: *mut c_double,
    nout: R_xlen_t,
    method: c_int,
    yleft: c_double,
    yright: c_double,
    f: c_double,
    na_rm: c_int,
) {
    let meth = appr_meth {
        ylow: yleft,
        yhigh: yright,
        f1: 1.0 - f,
        f2: f,
        kind: method,
        _na_rm: na_rm,
    };
    let mut i: R_xlen_t = 0;
    while i < nout {
        let v = *xout.add(i as usize);
        *yout.add(i as usize) = if v.is_nan() {
            v
        } else {
            approx1(v, x, y, nxy, &meth)
        };
        i += 1;
    }
}

pub unsafe fn ApproxTest(x: SEXP, y: SEXP, method: SEXP, f: SEXP, na_rm: SEXP) -> SEXP {
    let nx = XLENGTH(x);
    R_approxtest(
        REAL(x),
        REAL(y),
        nx,
        asInteger(method),
        asReal(f),
        asLogical(na_rm),
    );
    R_NilValue()
}

pub unsafe fn Approx(
    x: SEXP,
    y: SEXP,
    v: SEXP,
    method: SEXP,
    yleft: SEXP,
    yright: SEXP,
    f: SEXP,
    na_rm: SEXP,
) -> SEXP {
    let xout = Rf_protect(coerceVector(v, SEXPTYPE::REALSXP.0));
    let nx = XLENGTH(x);
    let nout = XLENGTH(xout);
    let yout = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, nout as c_int));
    R_approxfun(
        REAL(x),
        REAL(y),
        nx,
        REAL(xout),
        REAL(yout),
        nout,
        asInteger(method),
        asReal(yleft),
        asReal(yright),
        asReal(f),
        asLogical(na_rm),
    );
    Rf_unprotect(2);
    yout
}
