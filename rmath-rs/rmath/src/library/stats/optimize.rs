/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1998-2025  The R Core Team
 *  Copyright (C) 1995, 1996  Robert Gentleman and Ross Ihaka
 *  Copyright (C) 2003-2004  The R Foundation
 *
 *  Ported to Rust from r-source/src/library/stats/src/optimize.c
 *
 *  Implements Brent's method for one-dimensional minimization (optimize())
 *  and one-dimensional root finding (uniroot() / zeroin2 wrapper).
 */

use std::ffi::CString;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::main::errors::{Rf_error, Rf_warning};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

use crate::attrib_core::{R_NamesSymbol, setAttrib};
use crate::library::stats::zeroin::R_zeroin2;

// ---------------------------------------------------------------------------
// SETCADR helper
// ---------------------------------------------------------------------------

unsafe fn SETCADR(x: SEXP, y: SEXP) {
    if !x.is_null() {
        let cdr = CDR(x);
        if !cdr.is_null() {
            (*cdr).data.listsxp.carval = y;
        }
    }
}

// ---------------------------------------------------------------------------
// CallInfo -- callback state for evaluating R functions
// ---------------------------------------------------------------------------

struct CallInfo {
    R_fcall: SEXP,
    R_env: SEXP,
}

// ---------------------------------------------------------------------------
// fcn1 -- evaluate an R function at a single point, return double.
// Used by Brent_fmin (optimize / 1D minimization).
// ---------------------------------------------------------------------------

unsafe extern "C" fn fcn1(x: c_double, arg_info: *mut std::ffi::c_void) -> c_double {
    let info = &mut *(arg_info as *mut CallInfo);
    let sx = Rf_ScalarReal(x);
    let _sx_guard = protect(sx);
    SETCADR(info.R_fcall, sx);
    let s = eval(info.R_fcall, info.R_env);
    let _s_guard = protect(s);

    scalar_callback_value(s, b"invalid function value in 'optimize'\0".as_ptr() as *const _)
}

// ---------------------------------------------------------------------------
// fcn2 -- evaluate an R function at a single point, return double.
// Used by zeroin2 (uniroot / 1D root finding).  Identical logic to fcn1
// except for the error message.
// ---------------------------------------------------------------------------

unsafe extern "C" fn fcn2(x: c_double, arg_info: *mut std::ffi::c_void) -> c_double {
    let info = &mut *(arg_info as *mut CallInfo);
    let sx = Rf_ScalarReal(x);
    let _sx_guard = protect(sx);
    SETCADR(info.R_fcall, sx);
    let s = eval(info.R_fcall, info.R_env);
    let _s_guard = protect(s);

    scalar_callback_value(s, b"invalid function value in 'zeroin'\0".as_ptr() as *const _)
}

unsafe fn scalar_callback_value(s: SEXP, bad_value_message: *const c_char) -> c_double {
    match TYPEOF(s) {
        SEXPTYPE::INTSXP => {
            if LENGTH(s) != 1 {
                Rf_error(bad_value_message);
            }
            let value = *INTEGER(s);
            if value == NA_INTEGER {
                Rf_warning(b"NA replaced by maximum positive value\0".as_ptr() as *const _);
                f64::MAX
            } else {
                value as c_double
            }
        }
        SEXPTYPE::REALSXP => {
            if LENGTH(s) != 1 {
                Rf_error(bad_value_message);
            }
            let value = *REAL(s);
            if R_FINITE(value) {
                value
            } else if value == f64::NEG_INFINITY {
                Rf_warning(b"-Inf replaced by maximally negative value\0".as_ptr() as *const _);
                -f64::MAX
            } else {
                Rf_warning(
                    if ISNAN(value) {
                        b"NA/NaN replaced by maximum positive value\0".as_ptr()
                    } else {
                        b"Inf replaced by maximum positive value\0".as_ptr()
                    } as *const _,
                );
                f64::MAX
            }
        }
        _ => {
            Rf_error(bad_value_message);
            0.0
        }
    }
}

// ---------------------------------------------------------------------------
// Brent_fmin -- Brent's method for 1D minimization
//
// An approximation x to the point where f attains a minimum on the interval
// (ax, bx) is determined.
//
// The method used is a combination of golden section search and successive
// parabolic interpolation.  Convergence is never much slower than that for
// a Fibonacci search.  If f has a continuous second derivative which is
// positive at the minimum (which is not at ax or bx), then convergence is
// superlinear, and usually of the order of about 1.324....
//
// This is a slightly modified version of the Algol 60 procedure localmin
// given in Richard Brent, Algorithms for Minimization without Derivatives,
// Prentice-Hall, Inc. (1973).
// ---------------------------------------------------------------------------

pub unsafe fn Brent_fmin(
    ax: c_double,
    bx: c_double,
    f: unsafe extern "C" fn(c_double, *mut std::ffi::c_void) -> c_double,
    info: *mut std::ffi::c_void,
    tol: c_double,
) -> c_double {
    /* c is the squared inverse of the golden ratio */
    let c = (3.0 - libm::sqrt(5.0)) * 0.5;

    /* eps is approximately the square root of the relative machine precision */
    let mut eps = f64::EPSILON;
    let mut tol1 = eps + 1.0; /* the smallest 1.000... > 1 */
    eps = libm::sqrt(eps);

    let mut a = ax;
    let mut b = bx;
    let mut v = a + c * (b - a);
    let mut w = v;
    let mut x = v;

    let mut d = 0.0;
    let mut e = 0.0;
    let mut fx = f(x, info);
    let mut fv = fx;
    let mut fw = fx;
    let tol3 = tol / 3.0;

    /* main loop */
    loop {
        let xm = (a + b) * 0.5;
        tol1 = eps * libm::fabs(x) + tol3;
        let t2 = tol1 * 2.0;

        if libm::fabs(x - xm) <= t2 - (b - a) * 0.5 {
            break;
        }

        let mut p = 0.0;
        let mut q = 0.0;
        let mut r = 0.0;
        if libm::fabs(e) > tol1 {
            r = (x - w) * (fx - fv);
            q = (x - v) * (fx - fw);
            p = (x - v) * q - (x - w) * r;
            q = (q - r) * 2.0;
            if q > 0.0 {
                p = -p;
            } else {
                q = -q;
            }
            r = e;
            e = d;
        }

        if libm::fabs(p) >= libm::fabs(q * 0.5 * r)
            || p <= q * (a - x)
            || p >= q * (b - x)
        {
            if x < xm {
                e = b - x;
            } else {
                e = a - x;
            }
            d = c * e;
        } else {
            d = p / q;
            let u = x + d;
            if u - a < t2 || b - u < t2 {
                d = if x >= xm { -tol1 } else { tol1 };
            }
        }

        let u = if libm::fabs(d) >= tol1 {
            x + d
        } else if d > 0.0 {
            x + tol1
        } else {
            x - tol1
        };

        let fu = f(u, info);

        if fu <= fx {
            if u < x {
                b = x;
            } else {
                a = x;
            }
            v = w;
            w = x;
            x = u;
            fv = fw;
            fw = fx;
            fx = fu;
        } else {
            if u < x {
                a = u;
            } else {
                b = u;
            }
            if fu <= fw || w == x {
                v = w;
                fv = fw;
                w = u;
                fw = fu;
            } else if fu <= fv || v == x || v == w {
                v = u;
                fv = fu;
            }
        }
    }

    x
}

// ---------------------------------------------------------------------------
// Helper: asReal
// ---------------------------------------------------------------------------

unsafe fn as_real(x: SEXP) -> c_double {
    crate::main::coerce::asReal(x)
}

// ---------------------------------------------------------------------------
// Helper: asInteger
// ---------------------------------------------------------------------------

unsafe fn as_integer(x: SEXP) -> c_int {
    crate::main::coerce::asInteger(x)
}

// ---------------------------------------------------------------------------
// Helper: isFunction
// ---------------------------------------------------------------------------

unsafe fn is_function(x: SEXP) -> bool {
    if x.is_null() {
        return false;
    }
    let t = TYPEOF(x);
    t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP
}

// ---------------------------------------------------------------------------
// External declarations
// ---------------------------------------------------------------------------

unsafe fn eval(call: SEXP, rho: SEXP) -> SEXP {
    crate::eval::eval::Rf_eval(call, rho)
}

// ---------------------------------------------------------------------------
// do_fmin -- 1D minimization (optimize)
//
// Called from optimize() as:
//   .External2(C_do_fmin, function(arg) +/- f(arg, ...), lower, upper, tol)
// ---------------------------------------------------------------------------

pub unsafe fn do_fmin(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let mut args = CDR(args);

    /* the function to be minimized */
    let v = CAR(args);
    if !is_function(v) {
        Rf_error(b"attempt to minimize non-function\0".as_ptr() as *const _);
    }
    args = CDR(args);

    /* xmin */
    let xmin = as_real(CAR(args));
    if !R_FINITE(xmin) {
        Rf_error(b"invalid 'xmin' value\0".as_ptr() as *const _);
    }
    args = CDR(args);

    /* xmax */
    let xmax = as_real(CAR(args));
    if !R_FINITE(xmax) {
        Rf_error(b"invalid 'xmax' value\0".as_ptr() as *const _);
    }
    if xmin >= xmax {
        Rf_error(b"'xmin' not less than 'xmax'\0".as_ptr() as *const _);
    }
    args = CDR(args);

    /* tol */
    let tol = as_real(CAR(args));
    if !R_FINITE(tol) || tol <= 0.0 {
        Rf_error(b"invalid 'tol' value\0".as_ptr() as *const _);
    }

    let mut info = CallInfo {
        R_fcall: ptr::null_mut(),
        R_env: rho,
    };
    info.R_fcall = Rf_lang2(v, R_NilValue());
    let _fcall_guard = protect(info.R_fcall);
    let res = Rf_allocVector(SEXPTYPE::REALSXP, 1);
    let _res_guard = protect(res);
    *REAL(res) = Brent_fmin(xmin, xmax, fcn1, &mut info as *mut CallInfo as *mut std::ffi::c_void, tol);
    res
}

// ---------------------------------------------------------------------------
// do_zeroin2 -- 1D root finding (uniroot)
//
// zeroin2(f, ax, bx, f.ax, f.bx, tol, maxiter)
// ---------------------------------------------------------------------------

pub unsafe fn do_zeroin2(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let mut args = CDR(args);

    /* the function to be minimized */
    let v = CAR(args);
    if !is_function(v) {
        Rf_error(b"attempt to minimize non-function\0".as_ptr() as *const _);
    }
    args = CDR(args);

    /* xmin */
    let xmin = as_real(CAR(args));
    if !R_FINITE(xmin) {
        Rf_error(b"invalid 'xmin' value\0".as_ptr() as *const _);
    }
    args = CDR(args);

    /* xmax */
    let xmax = as_real(CAR(args));
    if !R_FINITE(xmax) {
        Rf_error(b"invalid 'xmax' value\0".as_ptr() as *const _);
    }
    if xmin >= xmax {
        Rf_error(b"'xmin' not less than 'xmax'\0".as_ptr() as *const _);
    }
    args = CDR(args);

    /* f(ax) = f(xmin) */
    let f_ax = as_real(CAR(args));
    if f_ax.is_nan() && !f_ax.is_infinite() {
        // ISNA check (NA is NaN with specific bit pattern; approximate here)
        Rf_error(b"NA value for 'f.lower' is not allowed\0".as_ptr() as *const _);
    }
    args = CDR(args);

    /* f(bx) = f(xmax) */
    let f_bx = as_real(CAR(args));
    if f_bx.is_nan() && !f_bx.is_infinite() {
        Rf_error(b"NA value for 'f.upper' is not allowed\0".as_ptr() as *const _);
    }
    args = CDR(args);

    /* tol */
    let mut tol = as_real(CAR(args));
    if !R_FINITE(tol) || tol <= 0.0 {
        Rf_error(b"invalid 'tol' value\0".as_ptr() as *const _);
    }
    args = CDR(args);

    /* maxiter */
    let mut iter = as_integer(CAR(args));
    if iter <= 0 {
        Rf_error(b"'maxiter' must be positive\0".as_ptr() as *const _);
    }

    let mut info = CallInfo {
        R_fcall: ptr::null_mut(),
        R_env: rho,
    };
    info.R_fcall = Rf_lang2(v, R_NilValue());
    let _fcall_guard = protect(info.R_fcall);
    let res = Rf_allocVector(SEXPTYPE::REALSXP, 3);
    let _res_guard = protect(res);

    let root = R_zeroin2(
        xmin,
        xmax,
        f_ax,
        f_bx,
        fcn2,
        &mut info as *mut CallInfo as *mut std::ffi::c_void,
        &mut tol,
        &mut iter,
    );

    *REAL(res) = root;
    *REAL(res).add(1) = iter as c_double;
    *REAL(res).add(2) = tol;

    res
}

// ===========================================================================
// nlm -- General Nonlinear Optimization (Dennis-Schnabel)
// ===========================================================================

const FT_SIZE: c_int = 5;

// ---------------------------------------------------------------------------
// FTable entry -- stores computed function values
// ---------------------------------------------------------------------------

struct FTableEntry {
    fval: c_double,
    x: *mut c_double,
    grad: *mut c_double,
    hess: *mut c_double,
}

// ---------------------------------------------------------------------------
// FunctionInfo -- state for the nlm optimizer
// ---------------------------------------------------------------------------

struct FunctionInfo {
    R_fcall: SEXP,
    R_env: SEXP,
    have_gradient: c_int,
    have_hessian: c_int,
    FT_size: c_int,
    FT_last: c_int,
    Ftable: *mut FTableEntry,
}

// ---------------------------------------------------------------------------
// Helper: allocate a vector of doubles (replaces R_alloc)
// ---------------------------------------------------------------------------

unsafe fn vect(n: c_int) -> *mut c_double {
    let layout = std::alloc::Layout::array::<c_double>(n as usize)
        .unwrap_or_else(|_| std::alloc::Layout::new::<c_double>());
    let ptr = std::alloc::alloc(layout) as *mut c_double;
    if ptr.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    ptr
}

// ---------------------------------------------------------------------------
// FT_init -- initialize the function-value cache table
// ---------------------------------------------------------------------------

unsafe fn FT_init(n: c_int, ft_size: c_int, state: &mut FunctionInfo) {
    let have_gradient = state.have_gradient;
    let have_hessian = state.have_hessian;

    let mut ftable: Vec<FTableEntry> = Vec::with_capacity(ft_size as usize);
    for _ in 0..ft_size {
        ftable.push(FTableEntry {
            fval: 0.0,
            x: ptr::null_mut(),
            grad: ptr::null_mut(),
            hess: ptr::null_mut(),
        });
    }
    let ftable_ptr = ftable.leak().as_mut_ptr();

    for i in 0..ft_size as usize {
        let xi = vect(n);
        for j in 0..n as usize {
            *xi.add(j) = f64::MAX;
        }
    }

    state.Ftable = ftable_ptr;
    state.FT_size = ft_size;
    state.FT_last = -1;
}

// ---------------------------------------------------------------------------
// FT_store -- store an entry in the function-value cache
// ---------------------------------------------------------------------------

unsafe fn FT_store(
    n: c_int,
    f: c_double,
    x: *const c_double,
    grad: *const c_double,
    hess: *const c_double,
    state: &mut FunctionInfo,
) {
    let ind = ((state.FT_last + 1) % state.FT_size) as usize;
    state.FT_last += 1;
    (*state.Ftable.add(ind)).fval = f;
    std::ptr::copy_nonoverlapping(x, (*state.Ftable.add(ind)).x, n as usize);
    if !grad.is_null() {
        std::ptr::copy_nonoverlapping(grad, (*state.Ftable.add(ind)).grad, n as usize);
        if !hess.is_null() {
            std::ptr::copy_nonoverlapping(hess, (*state.Ftable.add(ind)).hess, (n * n) as usize);
        }
    }
}

// ---------------------------------------------------------------------------
// FT_lookup -- check for stored values in the function-value cache.
// Returns the index in the table, or -1 for failure.
// ---------------------------------------------------------------------------

unsafe fn FT_lookup(n: c_int, x: *const c_double, state: &FunctionInfo) -> c_int {
    let ft_last = state.FT_last;
    let ft_size = state.FT_size;
    let ftable = state.Ftable;

    for i in 0..ft_size {
        let mut ind = (ft_last - i) % ft_size;
        if ind < 0 {
            ind += ft_size;
        }
    }
    -1
}

// ---------------------------------------------------------------------------
// fcn -- objective function callback for nlm
// ---------------------------------------------------------------------------

unsafe extern "C" fn fcn(
    n: c_int,
    x: *mut c_double,
    f_out: *mut c_double,
    arg_state: *mut std::ffi::c_void,
) {
    let state = &mut *(arg_state as *mut FunctionInfo);
    let R_fcall = state.R_fcall;

    let idx = FT_lookup(n, x, state);
    if idx >= 0 {
        *f_out = (*state.Ftable.add(idx as usize)).fval;
        return;
    }

    /* calculate for a new value of x */
    let s = Rf_allocVector(SEXPTYPE::REALSXP, n);
    SETCADR(R_fcall, s);
    for i in 0..n {
        let value = *x.add(i as usize);
        if !R_FINITE(value) {
            Rf_error(b"non-finite value supplied by 'nlm'\0".as_ptr() as *const _);
        }
        *REAL(s).add(i as usize) = value;
    }

    let s = eval(state.R_fcall, state.R_env);
    let _s_guard = protect(s);
    *f_out = scalar_callback_value(
        s,
        b"invalid function value in 'nlm' optimizer\0".as_ptr() as *const _,
    );

    let mut g: *mut c_double = ptr::null_mut();
    let mut h: *mut c_double = ptr::null_mut();
    let mut guards = Vec::new();

    if state.have_gradient != 0 {
        let grad_sym = crate::sexp::symbol::Rf_install(b"gradient\0".as_ptr() as *const c_char);
        let gv = getAttrib(s, grad_sym);
        let coerced = coerceVector(gv, SEXPTYPE::REALSXP.as_c_int());
        guards.push(protect(coerced));
        g = REAL(coerced);
        if state.have_hessian != 0 {
            let hess_sym = crate::sexp::symbol::Rf_install(b"hessian\0".as_ptr() as *const c_char);
            let hv = getAttrib(s, hess_sym);
            let coerced = coerceVector(hv, SEXPTYPE::REALSXP.as_c_int());
            guards.push(protect(coerced));
            h = REAL(coerced);
        }
    }

    FT_store(n, *f_out, x, g, h, state);
}

// ---------------------------------------------------------------------------
// getAttrib wrapper
// ---------------------------------------------------------------------------

unsafe fn getAttrib(x: SEXP, what: SEXP) -> SEXP {
    crate::attrib_core::getAttrib(x, what)
}

// ---------------------------------------------------------------------------
// coerceVector wrapper
// ---------------------------------------------------------------------------

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::main::coerce::coerceVector(x, type_)
}

// ---------------------------------------------------------------------------
// Cd1fcn -- gradient callback for nlm (retrieves from cache)
// ---------------------------------------------------------------------------

unsafe extern "C" fn Cd1fcn(
    n: c_int,
    x: *mut c_double,
    g: *mut c_double,
    arg_state: *mut std::ffi::c_void,
) {
    let state = &mut *(arg_state as *mut FunctionInfo);

    let mut ind = FT_lookup(n, x, state);
    if ind < 0 {
        /* shouldn't happen */
        fcn(n, x, g, arg_state);
        ind = FT_lookup(n, x, state);
        if ind < 0 {
            Rf_error(
                b"function value caching for optimization is seriously confused\0".as_ptr()
                    as *const _,
            );
        }
    }
    std::ptr::copy_nonoverlapping((*state.Ftable.add(ind as usize)).grad, g, n as usize);
}

// ---------------------------------------------------------------------------
// Cd2fcn -- Hessian callback for nlm (retrieves from cache)
// ---------------------------------------------------------------------------

unsafe extern "C" fn Cd2fcn(
    nr: c_int,
    n: c_int,
    x: *mut c_double,
    h: *mut c_double,
    arg_state: *mut std::ffi::c_void,
) {
    let state = &mut *(arg_state as *mut FunctionInfo);

    let mut ind = FT_lookup(n, x, state);
    if ind < 0 {
        /* shouldn't happen */
        fcn(n, x, h, arg_state);
        ind = FT_lookup(n, x, state);
        if ind < 0 {
            Rf_error(
                b"function value caching for optimization is seriously confused\0".as_ptr()
                    as *const _,
            );
        }
    }
    /* fill in lower triangle only */
    for j in 0..n {
        std::ptr::copy_nonoverlapping(
            (*state.Ftable.add(ind as usize)).hess.add((j * (n + 1)) as usize),
            h.add((j * (n + 1)) as usize),
            (n - j) as usize,
        );
    }
}

// ---------------------------------------------------------------------------
// fixparam -- extract a numeric vector from an SEXP parameter
// ---------------------------------------------------------------------------

unsafe fn fixparam(p: SEXP, n: &mut c_int) -> *mut c_double {
    if !is_numeric(p) {
        Rf_error(b"numeric parameter expected\0".as_ptr() as *const _);
    }

    if *n != 0 {
        if LENGTH(p) != *n {
            Rf_error(b"conflicting parameter lengths\0".as_ptr() as *const _);
        }
    } else {
        if LENGTH(p) <= 0 {
            Rf_error(b"invalid parameter length\0".as_ptr() as *const _);
        }
    }

    let x = vect(*n);

    if TYPEOF(p) == SEXPTYPE::LGLSXP || TYPEOF(p) == SEXPTYPE::INTSXP {
        for i in 0..*n as usize {
            let v = *INTEGER(p).add(i);
            if v == NA_INTEGER {
                Rf_error(b"missing value in parameter\0".as_ptr() as *const _);
            }
        }
    } else if TYPEOF(p) == SEXPTYPE::REALSXP {
        for i in 0..*n as usize {
            let v = *REAL(p).add(i);
            if !R_FINITE(v) {
                Rf_error(b"missing value in parameter\0".as_ptr() as *const _);
            }
        }
    } else {
        Rf_error(b"invalid parameter type\0".as_ptr() as *const _);
    }
    x
}

// ---------------------------------------------------------------------------
// isNumeric helper
// ---------------------------------------------------------------------------

unsafe fn is_numeric(x: SEXP) -> bool {
    if x.is_null() {
        return false;
    }
    let t = TYPEOF(x);
    t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP || t == SEXPTYPE::LGLSXP
}

// ---------------------------------------------------------------------------
// asLogical helper
// ---------------------------------------------------------------------------

unsafe fn as_logical(x: SEXP) -> c_int {
    crate::main::coerce::asLogical(x)
}

// ---------------------------------------------------------------------------
// opterror -- fatal errors from nlm
// ---------------------------------------------------------------------------

fn opterror(nerr: c_int) -> ! {
    match nerr {
        -1 => {
            let msg = CString::new("non-positive number of parameters in nlm").unwrap_or_default();
            unsafe { Rf_error(msg.as_ptr()) };
            std::process::abort()
        }
    }
}

// ---------------------------------------------------------------------------
// optcode -- warnings from nlm
// ---------------------------------------------------------------------------

fn optcode(code: c_int) {
    match code {
        1 => {
            eprintln!("Relative gradient close to zero.");
            eprintln!("Current iterate is probably solution.");
        }
    }
    eprintln!();
}

// ---------------------------------------------------------------------------
// allocMatrix helper -- allocate a matrix SEXP
// ---------------------------------------------------------------------------

unsafe fn alloc_matrix(sexptype: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    let ans = Rf_allocVector(sexptype, nrow * ncol);
    let _ans_guard = protect(ans);
    let dim = Rf_allocVector(SEXPTYPE::INTSXP, 2);
    let _dim_guard = protect(dim);
    *INTEGER(dim) = nrow;
    *INTEGER(dim).add(1) = ncol;
    crate::attrib_core::setAttrib(ans, crate::attrib_core::R_DimSymbol(), dim);
    ans
}

// ---------------------------------------------------------------------------
// nlm -- General Nonlinear Optimization (Dennis-Schnabel optif9)
//
// .Internal(
//   nlm(function(x) f(x, ...), p, hessian, typsize, fscale,
//       msg, ndigit, gradtol, stepmax, steptol, iterlim)
// )
// ---------------------------------------------------------------------------

pub unsafe fn nlm(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let mut args = CDR(args);

    let mut state = FunctionInfo {
        R_fcall: ptr::null_mut(),
        R_env: rho,
        have_gradient: 0,
        have_hessian: 0,
        FT_size: FT_SIZE,
        FT_last: -1,
        Ftable: ptr::null_mut(),
    };

    /* the function to be minimized */
    let v = CAR(args);
    if !is_function(v) {
        Rf_error(b"attempt to minimize non-function\0".as_ptr() as *const _);
    }
    state.R_fcall = Rf_lang2(v, R_NilValue());
    let _fcall_guard = protect(state.R_fcall);
    args = CDR(args);

    /* `p' : initial parameter value */
    let mut n: c_int = 0;
    let x = fixparam(CAR(args), &mut n);
    args = CDR(args);

    /* `hessian' : H. required? */
    let mut want_hessian = as_logical(CAR(args));
    if want_hessian == NA_INTEGER {
        want_hessian = 0;
    }
    args = CDR(args);

    /* `typsize' : typical size of parameter elements */
    let typsiz = fixparam(CAR(args), &mut n);
    args = CDR(args);

    /* `fscale' : expected function size */
    let fscale = as_real(CAR(args));
    if fscale.is_nan() {
        Rf_error(b"invalid NA value in parameter\0".as_ptr() as *const _);
    }
    args = CDR(args);

    /* `msg' (bit pattern) */
    let omsg = as_integer(CAR(args));
    let mut msg = omsg;
    if msg == NA_INTEGER {
        Rf_error(b"invalid NA value in parameter\0".as_ptr() as *const _);
    }
    args = CDR(args);

    let ndigit = as_integer(CAR(args));
    if ndigit == NA_INTEGER {
        Rf_error(b"invalid NA value in parameter\0".as_ptr() as *const _);
    }
    args = CDR(args);

    let gradtl = as_real(CAR(args));
    if gradtl.is_nan() {
        Rf_error(b"invalid NA value in parameter\0".as_ptr() as *const _);
    }
    args = CDR(args);

    let stepmx = as_real(CAR(args));
    if stepmx.is_nan() {
        Rf_error(b"invalid NA value in parameter\0".as_ptr() as *const _);
    }
    args = CDR(args);

    let steptol = as_real(CAR(args));
    if steptol.is_nan() {
        Rf_error(b"invalid NA value in parameter\0".as_ptr() as *const _);
    }
    args = CDR(args);

    /* `iterlim' (def. 100) */
    let itnlim = as_integer(CAR(args));
    if itnlim == NA_INTEGER {
        Rf_error(b"invalid NA value in parameter\0".as_ptr() as *const _);
    }

    state.R_env = rho;

    /* force one evaluation to check for the gradient and hessian */
    let mut iagflg: c_int = 0;
    let mut iahflg: c_int = 0;
    state.have_gradient = 0;
    state.have_hessian = 0;

    let r_gradient_symbol = crate::sexp::symbol::Rf_install(b"gradient\0".as_ptr() as *const c_char);
    let r_hessian_symbol = crate::sexp::symbol::Rf_install(b"hessian\0".as_ptr() as *const c_char);

    let vv = Rf_allocVector(SEXPTYPE::REALSXP, n);
    for i in 0..n {
        *REAL(vv).add(i as usize) = *x.add(i as usize);
    }
    SETCADR(state.R_fcall, vv);
    let value = eval(state.R_fcall, state.R_env);
    let _value_guard = protect(value);

    let gv = getAttrib(value, r_gradient_symbol);
    if Rf_isNull(gv) == 0 {
        if LENGTH(gv) == n && (TYPEOF(gv) == SEXPTYPE::REALSXP || TYPEOF(gv) == SEXPTYPE::INTSXP) {
            iagflg = 1;
            state.have_gradient = 1;
            let hv = getAttrib(value, r_hessian_symbol);
            if Rf_isNull(hv) == 0 {
                if LENGTH(hv) == (n * n)
                    && (TYPEOF(hv) == SEXPTYPE::REALSXP || TYPEOF(hv) == SEXPTYPE::INTSXP)
                {
                    iahflg = 1;
                    state.have_hessian = 1;
                } else {
                    Rf_warning(
                        b"hessian supplied is of the wrong length or mode, so ignored\0".as_ptr()
                            as *const _,
                    );
                }
            }
        } else {
            Rf_warning(
                b"gradient supplied is of the wrong length or mode, so ignored\0".as_ptr()
                    as *const _,
            );
        }
    }

    if ((msg / 4) % 2) != 0 && iahflg == 0 {
        msg -= 4;
    }
    if ((msg / 2) % 2) != 0 && iagflg == 0 {
        msg -= 2;
    }
    FT_init(n, FT_SIZE, &mut state);

    let method: c_int = 1;
    let iexp: c_int = if iahflg != 0 { 0 } else { 1 };
    let mut dlt: c_double = 1.0;

    let xpls = vect(n);
    let gpls = vect(n);
    let a = vect(n * n);
    let wrk = vect(8 * n);
    let mut fpls: c_double = 0.0;
    let mut code: c_int = 0;
    let mut itncnt: c_int = 0;

    /* Call optif9 */
    crate::appl::uncmin::optif9(
        n,
        n,
        x,
        fcn,
        Cd1fcn,
        Cd2fcn,
        &mut state as *mut FunctionInfo as *mut std::ffi::c_void,
        typsiz,
        fscale,
        method,
        iexp,
        &mut msg,
        ndigit,
        itnlim,
        iagflg,
        iahflg,
        dlt,
        gradtl,
        stepmx,
        steptol,
        xpls,
        &mut fpls,
        gpls,
        &mut code,
        a,
        wrk,
        &mut itncnt,
    );

    if msg < 0 {
        opterror(msg);
    }
    if code != 0 && (omsg & 8) == 0 {
        optcode(code);
    }

    let output_len = if want_hessian != 0 { 6 } else { 5 };
    let value = Rf_allocVector(SEXPTYPE::VECSXP, output_len);
    let _value_guard = protect(value);
    let names = Rf_allocVector(SEXPTYPE::STRSXP, output_len);
    let _names_guard = protect(names);

    if want_hessian != 0 {
        crate::appl::uncmin::fdhess(
            n,
            xpls,
            fpls,
            fcn,
            &mut state as *mut FunctionInfo as *mut std::ffi::c_void,
            a,
            n,
            wrk,
            wrk.add(n as usize),
            ndigit,
            typsiz,
        );
        for i in 0..n {
            for j in 0..i {
                let a_ji = *a.add((j + i * n) as usize);
                *a.add((i + j * n) as usize) = a_ji;
            }
        }
    }

    let mut k: R_xlen_t = 0;

    SET_STRING_ELT(names, k, Rf_mkChar(b"minimum\0".as_ptr() as *const c_char));
    SET_VECTOR_ELT(value, k, Rf_ScalarReal(fpls));
    k += 1;

    SET_STRING_ELT(names, k, Rf_mkChar(b"estimate\0".as_ptr() as *const c_char));
    let est = Rf_allocVector(SEXPTYPE::REALSXP, n);
    for i in 0..n {
        *REAL(est).add(i as usize) = *xpls.add(i as usize);
    }
    SET_VECTOR_ELT(value, k, est);
    k += 1;

    SET_STRING_ELT(names, k, Rf_mkChar(b"gradient\0".as_ptr() as *const c_char));
    let gradv = Rf_allocVector(SEXPTYPE::REALSXP, n);
    for i in 0..n {
        *REAL(gradv).add(i as usize) = *gpls.add(i as usize);
    }
    SET_VECTOR_ELT(value, k, gradv);
    k += 1;

    if want_hessian != 0 {
        SET_STRING_ELT(names, k, Rf_mkChar(b"hessian\0".as_ptr() as *const c_char));
        let hess = alloc_matrix(SEXPTYPE::REALSXP.as_c_int(), n, n);
        let _hess_guard = protect(hess);
        for i in 0..(n * n) as usize {
            *REAL(hess).add(i) = *a.add(i);
        }
        SET_VECTOR_ELT(value, k, hess);
        k += 1;
    }

    SET_STRING_ELT(names, k, Rf_mkChar(b"code\0".as_ptr() as *const c_char));
    let codev = Rf_allocVector(SEXPTYPE::INTSXP, 1);
    *INTEGER(codev) = code;
    SET_VECTOR_ELT(value, k, codev);
    k += 1;

    /* added by Jim K Lindsey */
    SET_STRING_ELT(names, k, Rf_mkChar(b"iterations\0".as_ptr() as *const c_char));
    let iterv = Rf_allocVector(SEXPTYPE::INTSXP, 1);
    *INTEGER(iterv) = itncnt;
    SET_VECTOR_ELT(value, k, iterv);
    k += 1;

    setAttrib(value, R_NamesSymbol(), names);
    value
}
