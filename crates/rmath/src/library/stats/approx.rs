//! Linear and Step Function Interpolation
//! Port of r-source/src/library/stats/src/approx.c

use std::os::raw::{c_double, c_int};
use std::slice;

use crate::main::coerce::R_FINITE;
use crate::main::coerce::{asInteger, asLogical, asReal, coerceVector};
use crate::sexp::accessors::{REAL, TYPEOF, XLENGTH};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect as protect_sexp;

struct appr_meth {
    ylow: c_double,
    yhigh: c_double,
    f1: c_double,
    f2: c_double,
    kind: c_int,
    _na_rm: c_int,
}

fn approx1(v: c_double, x: &[c_double], y: &[c_double], meth: &appr_meth) -> c_double {
    if x.is_empty() {
        return f64::NAN;
    }

    let mut i = 0usize;
    let mut j = x.len() - 1;

    if v < x[i] {
        return meth.ylow;
    }
    if v > x[j] {
        return meth.yhigh;
    }

    while i < j - 1 {
        let ij = (i + j) / 2;
        if v < x[ij] {
            j = ij;
        } else {
            i = ij;
        }
    }

    if v == x[j] {
        return y[j];
    }
    if v == x[i] {
        return y[i];
    }

    if meth.kind == 1 {
        y[i] + (y[j] - y[i]) * ((v - x[i]) / (x[j] - x[i]))
    } else {
        (if meth.f1 != 0.0 { y[i] * meth.f1 } else { 0.0 })
            + (if meth.f2 != 0.0 { y[j] * meth.f2 } else { 0.0 })
    }
}

fn R_approxtest(x: &[c_double], y: &[c_double], method: c_int, f: c_double, na_rm: c_int) {
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
        for (xv, yv) in x.iter().zip(y.iter()) {
            if xv.is_nan() || yv.is_nan() {
                eprintln!("approx(): attempted to interpolate NA values");
            }
        }
    } else {
        for xv in x {
            if xv.is_nan() {
                eprintln!("approx(x,y, .., na.rm=FALSE): NA values in x are not allowed");
            }
        }
    }
}

fn R_approxfun(
    x: &[c_double],
    y: &[c_double],
    xout: &[c_double],
    yout: &mut [c_double],
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
    for (i, &v) in xout.iter().enumerate() {
        yout[i] = if v.is_nan() {
            v
        } else {
            approx1(v, x, y, &meth)
        };
    }
}

pub unsafe fn ApproxTest(x: SEXP, y: SEXP, method: SEXP, f: SEXP, na_rm: SEXP) -> SEXP {
    let nx = unsafe { XLENGTH(x) };
    let x_slice = unsafe { slice::from_raw_parts(REAL(x), nx as usize) };
    let y_slice = unsafe { slice::from_raw_parts(REAL(y), nx as usize) };
    R_approxtest(
        x_slice,
        y_slice,
        unsafe { asInteger(method) },
        unsafe { asReal(f) },
        unsafe { asLogical(na_rm) },
    );
    unsafe { R_NilValue() }
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
    let xout = unsafe { coerceVector(v, SEXPTYPE::REALSXP.as_c_int()) };
    let _xout_guard = protect_sexp(xout);
    let nx = unsafe { XLENGTH(x) };
    let nout = unsafe { XLENGTH(xout) };
    let yout = unsafe { Rf_allocVector(SEXPTYPE::REALSXP, nout as c_int) };
    let _yout_guard = protect_sexp(yout);
    let x_slice = unsafe { slice::from_raw_parts(REAL(x), nx as usize) };
    let y_slice = unsafe { slice::from_raw_parts(REAL(y), nx as usize) };
    let xout_slice = unsafe { slice::from_raw_parts(REAL(xout), nout as usize) };
    let yout_slice = unsafe { slice::from_raw_parts_mut(REAL(yout), nout as usize) };
    R_approxfun(
        x_slice,
        y_slice,
        xout_slice,
        yout_slice,
        unsafe { asInteger(method) },
        unsafe { asReal(yleft) },
        unsafe { asReal(yright) },
        unsafe { asReal(f) },
        unsafe { asLogical(na_rm) },
    );
    yout
}

/// GNU `approx(x, y, xout)`.
pub unsafe fn do_approx(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, INTEGER, SET_VECTOR_ELT};
        use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector3};
        use crate::sexp::protect::protect;
        let x = CAR(args);
        let y = CAR(CDR(args));
        let xout = CAR(CDR(CDR(args)));
        let xc = coerceVector(x, SEXPTYPE::REALSXP.as_c_int());
        let _xc = protect(xc);
        let yc = coerceVector(y, SEXPTYPE::REALSXP.as_c_int());
        let _yc = protect(yc);
        let n = XLENGTH(xc);
        let yleft = Rf_ScalarReal(*REAL(yc));
        let _yl = protect(yleft);
        let yright = Rf_ScalarReal(*REAL(yc).add((n - 1) as usize));
        let _yr = protect(yright);
        let method = Rf_ScalarInteger(1);
        let _m = protect(method);
        let f = Rf_ScalarReal(0.0);
        let _f = protect(f);
        let na_rm = Rf_ScalarInteger(1);
        let _na = protect(na_rm);
        let yout = Approx(xc, yc, xout, method, yleft, yright, f, na_rm);
        let _yo = protect(yout);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, xout);
        SET_VECTOR_ELT(result, 1, yout);
        crate::mainutils::essentials::set_string_names(result, &["x".to_string(), "y".to_string()]);
        let _ = INTEGER;
        result
    }
}

/// GNU `approxfun(x, y)` linear interpolator.
pub unsafe fn do_approxfun(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, SETTAG};
        use crate::sexp::constructors::{Rf_cons, Rf_lang2};
        use crate::sexp::protect::protect;
        use crate::sexp::symbol::Rf_install;
        let x = coerceVector(CAR(args), SEXPTYPE::REALSXP.as_c_int());
        let _x = protect(x);
        let y = coerceVector(CAR(CDR(args)), SEXPTYPE::REALSXP.as_c_int());
        let _y = protect(y);
        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), rho, R_NilValue());
        let _env = protect(env);
        crate::sexp::envir::defineVar(Rf_install(c"x".as_ptr()), x, env);
        crate::sexp::envir::defineVar(Rf_install(c"y".as_ptr()), y, env);
        let vsym = Rf_install(c"v".as_ptr());
        let formals = Rf_cons(crate::sexp::globals::R_MissingArg(), R_NilValue());
        SETTAG(formals, vsym);
        let body = Rf_lang2(Rf_install(c".approx_apply".as_ptr()), vsym);
        crate::mainutils::dstruct::mkCLOSXP(formals, body, env)
    }
}

/// Evaluate an `approxfun` closure at `v`.
pub unsafe fn do_approx_apply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, ENCLOS};
        use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarReal};
        use crate::sexp::protect::protect;
        use crate::sexp::symbol::Rf_install;
        let v = CAR(args);
        let env = ENCLOS(rho);
        let x = crate::sexp::envir::R_findVar(Rf_install(c"x".as_ptr()), env);
        let y = crate::sexp::envir::R_findVar(Rf_install(c"y".as_ptr()), env);
        let n = XLENGTH(x);
        let yleft = Rf_ScalarReal(*REAL(y));
        let _yl = protect(yleft);
        let yright = Rf_ScalarReal(*REAL(y).add((n - 1) as usize));
        let _yr = protect(yright);
        let method = Rf_ScalarInteger(1);
        let _m = protect(method);
        let f = Rf_ScalarReal(0.0);
        let _f = protect(f);
        let na_rm = Rf_ScalarInteger(1);
        let _na = protect(na_rm);
        Approx(x, y, v, method, yleft, yright, f, na_rm)
    }
}
