/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1999-2025  The R Core Team
 *
 *  Ported to Rust from r-source/src/library/stats/src/optim.c
 */

use std::ffi::CString;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// Re-exports of functions defined elsewhere
// ---------------------------------------------------------------------------

use crate::attrib_core::{R_DimNamesSymbol, R_NamesSymbol, getAttrib, setAttrib};

// ---------------------------------------------------------------------------
// Local SETCADR helper
// ---------------------------------------------------------------------------

unsafe fn SETCADR(x: SEXP, y: SEXP) {
    unsafe {
        if !x.is_null() {
            let cdr = CDR(x);
            if !cdr.is_null() {
                (*cdr).data.listsxp.carval = y;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: allocate a vector of doubles (replaces R_alloc)
// ---------------------------------------------------------------------------

unsafe fn vect(n: c_int) -> *mut c_double {
    unsafe {
        let layout = std::alloc::Layout::array::<c_double>(n as usize)
            .unwrap_or_else(|_| std::alloc::Layout::new::<c_double>());
        let ptr = std::alloc::alloc(layout) as *mut c_double;
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        ptr
    }
}

/// Allocate a vector of ints (replaces R_alloc for int arrays).
unsafe fn vect_int(n: c_int) -> *mut c_int {
    unsafe {
        let layout = std::alloc::Layout::array::<c_int>(n as usize)
            .unwrap_or_else(|_| std::alloc::Layout::new::<c_int>());
        let ptr = std::alloc::alloc(layout) as *mut c_int;
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        ptr
    }
}

// ---------------------------------------------------------------------------
// OptStruct -- optimization state
// ---------------------------------------------------------------------------

struct OptStruct {
    R_fcall: SEXP,
    R_gcall: SEXP,
    R_env: SEXP,
    ndeps: *mut c_double,
    fnscale: c_double,
    parscale: *mut c_double,
    usebounds: c_int,
    lower: *mut c_double,
    upper: *mut c_double,
    names: SEXP,
}

// ---------------------------------------------------------------------------
// getListElement -- get named element from a list
// ---------------------------------------------------------------------------

unsafe fn getListElement(list: SEXP, str: *const c_char) -> SEXP {
    unsafe {
        if TYPEOF(list) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let mut elmt = R_NilValue();
        let names = getAttrib(list, R_NamesSymbol());
        let len = LENGTH(list);
        let c_str = std::ffi::CStr::from_ptr(str);
        let target = c_str.to_bytes();

        for i in 0..len {
            let name_sexp = STRING_ELT(names, i as R_xlen_t);
            if name_sexp.is_null() {
                continue;
            }
            let name_ptr = CHAR(name_sexp);
            if name_ptr.is_null() {
                continue;
            }
            let name_bytes = std::ffi::CStr::from_ptr(name_ptr).to_bytes();
            if name_bytes == target {
                elmt = VECTOR_ELT(list, i as R_xlen_t);
                break;
            }
        }
        elmt
    }
}

// ---------------------------------------------------------------------------
// fminfn -- objective function callback for optim
// ---------------------------------------------------------------------------

unsafe extern "C" fn fminfn(n: c_int, p: *mut c_double, ex: *mut std::ffi::c_void) -> c_double {
    unsafe {
        let os = &mut *(ex as *mut OptStruct);
        let x = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let _x_guard = protect(x);
        if Rf_isNull((*os).names) == 0 {
            setAttrib(x, R_NamesSymbol(), (*os).names);
        }
        for i in 0..n {
            if !R_FINITE(*p.add(i as usize)) {
                let msg =
                    CString::new(format!("non-finite value supplied by optim")).unwrap_or_default();
                Rf_error(msg.as_ptr());
            }
            *REAL(x).add(i as usize) = *p.add(i as usize) * *(*os).parscale.add(i as usize);
        }
        SETCADR((*os).R_fcall, x);
        let s = eval((*os).R_fcall, (*os).R_env);
        let _s_guard = protect(s);
        let s = coerceVector(s, SEXPTYPE::REALSXP.as_c_int());
        let _coerced_guard = protect(s);
        if LENGTH(s) != 1 {
            let msg = CString::new(format!(
                "objective function in optim evaluates to length {} not 1",
                LENGTH(s)
            ))
            .unwrap_or_default();
            Rf_error(msg.as_ptr());
        }
        let val = *REAL(s).add(0) / (*os).fnscale;
        val
    }
}

// ---------------------------------------------------------------------------
// fmingr -- gradient callback for optim
// ---------------------------------------------------------------------------

unsafe extern "C" fn fmingr(
    n: c_int,
    p: *mut c_double,
    df: *mut c_double,
    ex: *mut std::ffi::c_void,
) {
    unsafe {
        let os = &mut *(ex as *mut OptStruct);

        if Rf_isNull((*os).R_gcall) == 0 {
            // Analytical derivatives
            let x = Rf_allocVector(SEXPTYPE::REALSXP, n);
            let _x_guard = protect(x);
            if Rf_isNull((*os).names) == 0 {
                setAttrib(x, R_NamesSymbol(), (*os).names);
            }
            for i in 0..n {
                if !R_FINITE(*p.add(i as usize)) {
                    let msg = CString::new(format!("non-finite value supplied by optim"))
                        .unwrap_or_default();
                    Rf_error(msg.as_ptr());
                }
                *REAL(x).add(i as usize) = *p.add(i as usize) * *(*os).parscale.add(i as usize);
            }
            SETCADR((*os).R_gcall, x);
            let s = eval((*os).R_gcall, (*os).R_env);
            let _s_guard = protect(s);
            let s = coerceVector(s, SEXPTYPE::REALSXP.as_c_int());
            let _coerced_guard = protect(s);
            if LENGTH(s) != n {
                let msg = CString::new(format!(
                    "gradient in optim evaluated to length {} not {}",
                    LENGTH(s),
                    n
                ))
                .unwrap_or_default();
                Rf_error(msg.as_ptr());
            }
            for i in 0..n {
                *df.add(i as usize) =
                    *REAL(s).add(i as usize) * *(*os).parscale.add(i as usize) / (*os).fnscale;
            }
        } else {
            // Numerical derivatives
            let x = Rf_allocVector(SEXPTYPE::REALSXP, n);
            let _x_guard = protect(x);
            setAttrib(x, R_NamesSymbol(), (*os).names);
            for i in 0..n {
                *REAL(x).add(i as usize) = *p.add(i as usize) * *(*os).parscale.add(i as usize);
            }
            SETCADR((*os).R_fcall, x);

            if (*os).usebounds == 0 {
                for i in 0..n {
                    let eps = *(*os).ndeps.add(i as usize);
                    *REAL(x).add(i as usize) =
                        (*p.add(i as usize) + eps) * *(*os).parscale.add(i as usize);
                    let val1 = {
                        let s1 = eval((*os).R_fcall, (*os).R_env);
                        let _s1_guard = protect(s1);
                        let s1 = coerceVector(s1, SEXPTYPE::REALSXP.as_c_int());
                        let _coerced_guard = protect(s1);
                        *REAL(s1).add(0) / (*os).fnscale
                    };
                    *REAL(x).add(i as usize) =
                        (*p.add(i as usize) - eps) * *(*os).parscale.add(i as usize);
                    let val2 = {
                        let s2 = eval((*os).R_fcall, (*os).R_env);
                        let _s2_guard = protect(s2);
                        let s2 = coerceVector(s2, SEXPTYPE::REALSXP.as_c_int());
                        let _coerced_guard = protect(s2);
                        *REAL(s2).add(0) / (*os).fnscale
                    };
                    *df.add(i as usize) = (val1 - val2) / (2.0 * eps);
                    if !R_FINITE(*df.add(i as usize)) {
                        let msg =
                            CString::new(format!("non-finite finite-difference value [{}]", i + 1))
                                .unwrap_or_default();
                        Rf_error(msg.as_ptr());
                    }
                    *REAL(x).add(i as usize) = *p.add(i as usize) * *(*os).parscale.add(i as usize);
                }
            } else {
                // usebounds
                for i in 0..n {
                    let mut epsused = *(*os).ndeps.add(i as usize);
                    let mut eps = epsused;
                    let mut tmp = *p.add(i as usize) + eps;
                    if tmp > *(*os).upper.add(i as usize) {
                        tmp = *(*os).upper.add(i as usize);
                        epsused = tmp - *p.add(i as usize);
                    }
                    *REAL(x).add(i as usize) = tmp * *(*os).parscale.add(i as usize);
                    let val1 = {
                        let s1 = eval((*os).R_fcall, (*os).R_env);
                        let _s1_guard = protect(s1);
                        let s1 = coerceVector(s1, SEXPTYPE::REALSXP.as_c_int());
                        let _coerced_guard = protect(s1);
                        *REAL(s1).add(0) / (*os).fnscale
                    };
                    tmp = *p.add(i as usize) - eps;
                    if tmp < *(*os).lower.add(i as usize) {
                        tmp = *(*os).lower.add(i as usize);
                        eps = *p.add(i as usize) - tmp;
                    }
                    *REAL(x).add(i as usize) = tmp * *(*os).parscale.add(i as usize);
                    let val2 = {
                        let s2 = eval((*os).R_fcall, (*os).R_env);
                        let _s2_guard = protect(s2);
                        let s2 = coerceVector(s2, SEXPTYPE::REALSXP.as_c_int());
                        let _coerced_guard = protect(s2);
                        *REAL(s2).add(0) / (*os).fnscale
                    };
                    *df.add(i as usize) = (val1 - val2) / (epsused + eps);
                    if !R_FINITE(*df.add(i as usize)) {
                        let msg =
                            CString::new(format!("non-finite finite-difference value [{}]", i + 1))
                                .unwrap_or_default();
                        Rf_error(msg.as_ptr());
                    }
                    *REAL(x).add(i as usize) = *p.add(i as usize) * *(*os).parscale.add(i as usize);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: asReal on an SEXP
// ---------------------------------------------------------------------------

unsafe fn as_real(x: SEXP) -> c_double {
    unsafe {
        if x.is_null() {
            return NA_REAL;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            return *REAL(x);
        }
        if t == SEXPTYPE::INTSXP {
            let v = *INTEGER(x);
            if v == NA_INTEGER {
                return NA_REAL;
            }
            return v as c_double;
        }
        if t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x);
            if v == NA_INTEGER {
                return NA_REAL;
            }
            return if v != 0 { 1.0 } else { 0.0 };
        }
        NA_REAL
    }
}

// ---------------------------------------------------------------------------
// Helper: asInteger on an SEXP
// ---------------------------------------------------------------------------

unsafe fn as_integer(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP {
            return *INTEGER(x);
        }
        if t == SEXPTYPE::REALSXP {
            let v = *REAL(x);
            if v.is_nan() || v < c_int::MIN as c_double || v > c_int::MAX as c_double {
                return NA_INTEGER;
            }
            return v as c_int;
        }
        if t == SEXPTYPE::LGLSXP {
            return *INTEGER(x);
        }
        NA_INTEGER
    }
}

// ---------------------------------------------------------------------------
// Helper: isFunction check
// ---------------------------------------------------------------------------

unsafe fn is_function(x: SEXP) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        let t = TYPEOF(x);
        t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP
    }
}

// ---------------------------------------------------------------------------
// Helper: isString check
// ---------------------------------------------------------------------------

unsafe fn is_string(x: SEXP) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        TYPEOF(x) == SEXPTYPE::STRSXP
    }
}

// ---------------------------------------------------------------------------
// External function declarations
// ---------------------------------------------------------------------------

unsafe fn eval(call: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::eval::eval::Rf_eval(call, rho) }
}

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    unsafe { crate::main::coerce::coerceVector(x, type_) }
}

unsafe fn duplicate(x: SEXP) -> SEXP {
    unsafe { crate::main::duplicate::duplicate(x) }
}

unsafe fn allocMatrix(sexptype: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    unsafe {
        let ans = Rf_allocVector(sexptype, nrow * ncol);
        let _ans_guard = protect(ans);
        let dim = Rf_allocVector(SEXPTYPE::INTSXP, 2);
        let _dim_guard = protect(dim);
        *INTEGER(dim) = nrow;
        *INTEGER(dim).add(1) = ncol;
        crate::attrib_core::setAttrib(ans, crate::attrib_core::R_DimSymbol(), dim);
        ans
    }
}

use crate::appl::lbfgsb::lbfgsb;
use crate::appl::optim::{cgmin, nmmin, vmmin};
use crate::nmath::dist::normal::norm_rand;

// ---------------------------------------------------------------------------
// genptry -- generate candidate point for simulated annealing (ported from
//            R source: src/appl/optim.c lines 48-77)
//
// Generates a candidate point by adding scaled Cauchy noise to each
// coordinate, evaluates the objective function at the candidate, and
// returns the function value.
// ---------------------------------------------------------------------------

unsafe fn genptry(
    n: c_int,
    x: *mut f64,
    xp: *mut f64,
    t: f64,
    os: *mut OptStruct,
    fminfn: Option<unsafe extern "C" fn(c_int, *mut f64, *mut std::ffi::c_void) -> f64>,
    ex: *mut std::ffi::c_void,
) -> f64 {
    unsafe {
        const E1: f64 = 1.7182818; /* exp(1) - 1 */

        let s = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let mut s_guard = protect_with_index_raw(s, "genptry candidate");

        for i in 0..n as usize {
            /* generate a Cauchy variate */
            let u1 = norm_rand();
            let u2 = norm_rand();
            let cauchy = u1 / u2;

            /* scale by temperature and current value */
            *REAL(s).add(i) = *x.add(i) + t * cauchy * (1.0 + E1 * (*x.add(i)).abs());
        }

        /* check that the new point is finite */
        for i in 0..n as usize {
            if !R_FINITE(*REAL(s).add(i)) {
                *REAL(s).add(i) = *x.add(i);
            }
        }

        /* copy to xp and evaluate */
        for i in 0..n as usize {
            *xp.add(i) = *REAL(s).add(i);
        }

        let fminfn = if let Some(f) = fminfn { f } else { return 0.0 };
        let y = fminfn(n, xp, ex);

        s_guard.reprotect_raw(s);
        y
    }
}

// ---------------------------------------------------------------------------
// samin -- simulated annealing optimization (ported from R source:
//           src/appl/optim.c lines 720-790)
//
// Simulated annealing using the Metropolis criterion. The function performs
// maxit iterations, each consisting of tmax function evaluations at a given
// temperature. The temperature decreases linearly from temp to near zero.
// ---------------------------------------------------------------------------

unsafe fn samin(
    n: c_int,
    sb: *mut f64,
    ybest: *mut f64,
    fminfn: Option<unsafe extern "C" fn(c_int, *mut f64, *mut std::ffi::c_void) -> f64>,
    maxit: c_int,
    tmax: c_int,
    temp: f64,
    trace: c_int,
    ex: *mut std::ffi::c_void,
) {
    unsafe {
        const BIG: f64 = 1e30;

        let os = ex as *mut OptStruct;
        let n_usize = n as usize;

        /* Allocate a candidate point */
        let xp = vect(n);

        let mut nacc = 0;
        let mut nfcnev = 0;

        /* Evaluate at the initial point */
        *ybest = if let Some(f) = fminfn {
            f(n, sb, ex)
        } else {
            0.0
        };
        nfcnev += 1;

        /* perform the annealing */
        for it in 0..maxit {
            let t = temp * (1.0 - it as f64 / maxit as f64);

            for j in 0..tmax {
                let ytry = genptry(n, sb, xp, t, os, fminfn, ex);
                nfcnev += 1;

                /* Metropolis acceptance step */
                let fac = ytry - *ybest;
                if fac < 0.0 {
                    /* accept -- new value is better */
                    for i in 0..n_usize {
                        *sb.add(i) = *xp.add(i);
                    }
                    *ybest = ytry;
                    nacc += 1;
                } else {
                    /* accept with probability exp(-fac/t) */
                    let p = if t > 0.0 && fac < BIG {
                        (-fac / t).exp()
                    } else {
                        0.0
                    };
                    let u = norm_rand(); /* uniform in (0,1) approximately */
                    if u < p {
                        for i in 0..n_usize {
                            *sb.add(i) = *xp.add(i);
                        }
                        *ybest = ytry;
                        nacc += 1;
                    }
                }

                if trace != 0 && (j % trace == 0 || j + 1 == tmax) {
                    eprintln!(
                        "samin(): iter = {} of maxit = {}; f_new = {}; t = {}",
                        it * tmax + j,
                        maxit * tmax,
                        ytry,
                        t
                    );
                }
            }
        }

        if trace != 0 {
            eprintln!(
                "samin() after {} iterations: fval = {}; nacc = {}; nfcnev = {}",
                maxit * tmax,
                *ybest,
                nacc,
                nfcnev
            );
        }
    }
}

// ---------------------------------------------------------------------------
// optim -- main optimization entry point
// ---------------------------------------------------------------------------

pub unsafe fn optim(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut args = CDR(args);
        let mut os = Box::new(OptStruct {
            R_fcall: ptr::null_mut(),
            R_gcall: ptr::null_mut(),
            R_env: rho,
            ndeps: ptr::null_mut(),
            fnscale: 1.0,
            parscale: ptr::null_mut(),
            usebounds: 0,
            lower: ptr::null_mut(),
            upper: ptr::null_mut(),
            names: R_NilValue(),
        });
        let os_ptr: *mut OptStruct = &mut *os;

        (*os_ptr).usebounds = 0;
        (*os_ptr).R_env = rho;

        let par = CAR(args);
        (*os_ptr).names = getAttrib(par, R_NamesSymbol());
        args = CDR(args);
        let fn_sexp = CAR(args);
        if !is_function(fn_sexp) {
            Rf_error(b"'fn' is not a function\0".as_ptr() as *const _);
        }
        args = CDR(args);
        let gr = CAR(args);
        args = CDR(args);
        let method = CAR(args);
        if !is_string(method) || LENGTH(method) != 1 {
            Rf_error(b"invalid 'method' argument\0".as_ptr() as *const _);
        }
        let method_str = CHAR(STRING_ELT(method, 0));
        let tn = std::ffi::CStr::from_ptr(method_str).to_bytes();
        args = CDR(args);
        let options = CAR(args);

        (*os_ptr).R_fcall = Rf_lang2(fn_sexp, R_NilValue());
        let _fcall_guard = protect((*os_ptr).R_fcall);

        let par = coerceVector(par, SEXPTYPE::REALSXP.as_c_int());
        let _par_guard = protect(par);
        let npar = LENGTH(par);
        let dpar = vect(npar);
        let opar = vect(npar);

        let trace = as_integer(getListElement(
            options,
            CString::new("trace").unwrap_or_default().as_ptr(),
        ));
        (*os_ptr).fnscale = as_real(getListElement(
            options,
            CString::new("fnscale").unwrap_or_default().as_ptr(),
        ));
        let tmp = getListElement(
            options,
            CString::new("parscale").unwrap_or_default().as_ptr(),
        );
        if LENGTH(tmp) != npar {
            Rf_error(b"'parscale' is of the wrong length\0".as_ptr() as *const _);
        }
        (*os_ptr).parscale = vect(npar);
        {
            let tmp = coerceVector(tmp, SEXPTYPE::REALSXP.as_c_int());
            let _tmp_guard = protect(tmp);
            for i in 0..npar {
                *(*os_ptr).parscale.add(i as usize) = *REAL(tmp).add(i as usize);
            }
        }
        for i in 0..npar {
            *dpar.add(i as usize) =
                *REAL(par).add(i as usize) / *(*os_ptr).parscale.add(i as usize);
        }

        let res = Rf_allocVector(SEXPTYPE::VECSXP, 5);
        let _res_guard = protect(res);
        let names = Rf_allocVector(SEXPTYPE::STRSXP, 5);
        let _names_guard = protect(names);
        SET_STRING_ELT(
            names,
            0,
            Rf_mkChar(CString::new("par").unwrap_or_default().as_ptr()),
        );
        SET_STRING_ELT(
            names,
            1,
            Rf_mkChar(CString::new("value").unwrap_or_default().as_ptr()),
        );
        SET_STRING_ELT(
            names,
            2,
            Rf_mkChar(CString::new("counts").unwrap_or_default().as_ptr()),
        );
        SET_STRING_ELT(
            names,
            3,
            Rf_mkChar(CString::new("convergence").unwrap_or_default().as_ptr()),
        );
        SET_STRING_ELT(
            names,
            4,
            Rf_mkChar(CString::new("message").unwrap_or_default().as_ptr()),
        );
        setAttrib(res, R_NamesSymbol(), names);

        let value = Rf_allocVector(SEXPTYPE::REALSXP, 1);
        let _value_guard = protect(value);
        let counts = Rf_allocVector(SEXPTYPE::INTSXP, 2);
        let _counts_guard = protect(counts);
        let countnames = Rf_allocVector(SEXPTYPE::STRSXP, 2);
        let _countnames_guard = protect(countnames);
        SET_STRING_ELT(
            countnames,
            0,
            Rf_mkChar(CString::new("function").unwrap_or_default().as_ptr()),
        );
        SET_STRING_ELT(
            countnames,
            1,
            Rf_mkChar(CString::new("gradient").unwrap_or_default().as_ptr()),
        );
        setAttrib(counts, R_NamesSymbol(), countnames);

        let conv = Rf_allocVector(SEXPTYPE::INTSXP, 1);
        let _conv_guard = protect(conv);
        let abstol = as_real(getListElement(
            options,
            CString::new("abstol").unwrap_or_default().as_ptr(),
        ));
        let reltol = as_real(getListElement(
            options,
            CString::new("reltol").unwrap_or_default().as_ptr(),
        ));
        let maxit = as_integer(getListElement(
            options,
            CString::new("maxit").unwrap_or_default().as_ptr(),
        ));
        if maxit == NA_INTEGER {
            Rf_error(b"'maxit' is not an integer\0".as_ptr() as *const _);
        }

        let mut fncount: c_int = 0;
        let mut grcount: c_int = 0;
        let mut ifail: c_int = 0;
        let mut val: c_double = 0.0;

        if tn == b"Nelder-Mead" {
            let alpha = as_real(getListElement(
                options,
                CString::new("alpha").unwrap_or_default().as_ptr(),
            ));
            let beta = as_real(getListElement(
                options,
                CString::new("beta").unwrap_or_default().as_ptr(),
            ));
            let gamm = as_real(getListElement(
                options,
                CString::new("gamma").unwrap_or_default().as_ptr(),
            ));
            nmmin(
                npar,
                dpar,
                opar,
                &mut val,
                fminfn,
                &mut ifail,
                abstol,
                reltol,
                os_ptr as *mut std::ffi::c_void,
                alpha,
                beta,
                gamm,
                trace,
                &mut fncount,
                maxit,
            );
            for i in 0..npar {
                *REAL(par).add(i as usize) =
                    *opar.add(i as usize) * *(*os_ptr).parscale.add(i as usize);
            }
            grcount = NA_INTEGER;
        } else if tn == b"SANN" {
            let tmax = as_integer(getListElement(
                options,
                CString::new("tmax").unwrap_or_default().as_ptr(),
            ));
            let temp = as_real(getListElement(
                options,
                CString::new("temp").unwrap_or_default().as_ptr(),
            ));
            let trace_val = if trace != 0 {
                as_integer(getListElement(
                    options,
                    CString::new("REPORT").unwrap_or_default().as_ptr(),
                ))
            } else {
                0
            };
            if tmax == NA_INTEGER || tmax < 1 {
                Rf_error(b"'tmax' is not a positive integer\0".as_ptr() as *const _);
            }
            if Rf_isNull(gr) == 0 {
                if !is_function(gr) {
                    Rf_error(b"'gr' is not a function\0".as_ptr() as *const _);
                }
                (*os_ptr).R_gcall = Rf_lang2(gr, R_NilValue());
            } else {
                (*os_ptr).R_gcall = R_NilValue();
            }
            let _gcall_guard = protect((*os_ptr).R_gcall);
            samin(
                npar,
                dpar,
                &mut val,
                Some(fminfn),
                maxit,
                tmax,
                temp,
                trace_val,
                os_ptr as *mut std::ffi::c_void,
            );
            for i in 0..npar {
                *REAL(par).add(i as usize) =
                    *dpar.add(i as usize) * *(*os_ptr).parscale.add(i as usize);
            }
            fncount = if npar > 0 { maxit } else { 1 };
            grcount = NA_INTEGER;
        } else if tn == b"BFGS" {
            let nREPORT = as_integer(getListElement(
                options,
                CString::new("REPORT").unwrap_or_default().as_ptr(),
            ));
            if Rf_isNull(gr) == 0 {
                if !is_function(gr) {
                    Rf_error(b"'gr' is not a function\0".as_ptr() as *const _);
                }
                (*os_ptr).R_gcall = Rf_lang2(gr, R_NilValue());
            } else {
                (*os_ptr).R_gcall = R_NilValue();
                let ndeps =
                    getListElement(options, CString::new("ndeps").unwrap_or_default().as_ptr());
                if LENGTH(ndeps) != npar {
                    Rf_error(b"'ndeps' is of the wrong length\0".as_ptr() as *const _);
                }
                (*os_ptr).ndeps = vect(npar);
                let ndeps = coerceVector(ndeps, SEXPTYPE::REALSXP.as_c_int());
                let _ndeps_guard = protect(ndeps);
                for i in 0..npar {
                    *(*os_ptr).ndeps.add(i as usize) = *REAL(ndeps).add(i as usize);
                }
            }
            let _gcall_guard = protect((*os_ptr).R_gcall);
            let mask = vect_int(npar);
            for i in 0..npar {
                *mask.add(i as usize) = 1;
            }
            vmmin(
                npar,
                dpar,
                &mut val,
                fminfn,
                fmingr,
                maxit,
                trace,
                mask,
                abstol,
                reltol,
                nREPORT,
                os_ptr as *mut std::ffi::c_void,
                &mut fncount,
                &mut grcount,
                &mut ifail,
            );
            for i in 0..npar {
                *REAL(par).add(i as usize) =
                    *dpar.add(i as usize) * *(*os_ptr).parscale.add(i as usize);
            }
        } else if tn == b"CG" {
            let type_val = as_integer(getListElement(
                options,
                CString::new("type").unwrap_or_default().as_ptr(),
            ));
            if Rf_isNull(gr) == 0 {
                if !is_function(gr) {
                    Rf_error(b"'gr' is not a function\0".as_ptr() as *const _);
                }
                (*os_ptr).R_gcall = Rf_lang2(gr, R_NilValue());
            } else {
                (*os_ptr).R_gcall = R_NilValue();
                let ndeps =
                    getListElement(options, CString::new("ndeps").unwrap_or_default().as_ptr());
                if LENGTH(ndeps) != npar {
                    Rf_error(b"'ndeps' is of the wrong length\0".as_ptr() as *const _);
                }
                (*os_ptr).ndeps = vect(npar);
                let ndeps = coerceVector(ndeps, SEXPTYPE::REALSXP.as_c_int());
                let _ndeps_guard = protect(ndeps);
                for i in 0..npar {
                    *(*os_ptr).ndeps.add(i as usize) = *REAL(ndeps).add(i as usize);
                }
            }
            let _gcall_guard = protect((*os_ptr).R_gcall);
            cgmin(
                npar,
                dpar,
                opar,
                &mut val,
                fminfn,
                fmingr,
                &mut ifail,
                abstol,
                reltol,
                os_ptr as *mut std::ffi::c_void,
                type_val,
                trace,
                &mut fncount,
                &mut grcount,
                maxit,
            );
            for i in 0..npar {
                *REAL(par).add(i as usize) =
                    *opar.add(i as usize) * *(*os_ptr).parscale.add(i as usize);
            }
        } else if tn == b"L-BFGS-B" {
            let nREPORT = as_integer(getListElement(
                options,
                CString::new("REPORT").unwrap_or_default().as_ptr(),
            ));
            let factr = as_real(getListElement(
                options,
                CString::new("factr").unwrap_or_default().as_ptr(),
            ));
            let pgtol = as_real(getListElement(
                options,
                CString::new("pgtol").unwrap_or_default().as_ptr(),
            ));
            let lmm = as_integer(getListElement(
                options,
                CString::new("lmm").unwrap_or_default().as_ptr(),
            ));
            if Rf_isNull(gr) == 0 {
                if !is_function(gr) {
                    Rf_error(b"'gr' is not a function\0".as_ptr() as *const _);
                }
                (*os_ptr).R_gcall = Rf_lang2(gr, R_NilValue());
            } else {
                (*os_ptr).R_gcall = R_NilValue();
                let ndeps =
                    getListElement(options, CString::new("ndeps").unwrap_or_default().as_ptr());
                if LENGTH(ndeps) != npar {
                    Rf_error(b"'ndeps' is of the wrong length\0".as_ptr() as *const _);
                }
                (*os_ptr).ndeps = vect(npar);
                let ndeps = coerceVector(ndeps, SEXPTYPE::REALSXP.as_c_int());
                let _ndeps_guard = protect(ndeps);
                for i in 0..npar {
                    *(*os_ptr).ndeps.add(i as usize) = *REAL(ndeps).add(i as usize);
                }
            }
            let _gcall_guard = protect((*os_ptr).R_gcall);
            args = CDR(args);
            let slower = CAR(args);
            args = CDR(args);
            let supper = CAR(args);

            let lower = vect(npar);
            let upper = vect(npar);
            let nbd = vect_int(npar);
            for i in 0..npar {
                *lower.add(i as usize) =
                    *REAL(slower).add(i as usize) / *(*os_ptr).parscale.add(i as usize);
                *upper.add(i as usize) =
                    *REAL(supper).add(i as usize) / *(*os_ptr).parscale.add(i as usize);
                if !R_FINITE(*lower.add(i as usize)) {
                    if !R_FINITE(*upper.add(i as usize)) {
                        *nbd.add(i as usize) = 0;
                    } else {
                        *nbd.add(i as usize) = 3;
                    }
                } else {
                    if !R_FINITE(*upper.add(i as usize)) {
                        *nbd.add(i as usize) = 1;
                    } else {
                        *nbd.add(i as usize) = 2;
                    }
                }
            }
            (*os_ptr).usebounds = 1;
            (*os_ptr).lower = lower;
            (*os_ptr).upper = upper;

            // Allocate work arrays for lbfgsb (Fortran-style stateful API)
            // wa size from R source: (2*m*n + 5*n + 11*m*m + 8*m + 1)
            let wa_len = (2 * lmm * npar + 5 * npar + 11 * lmm * lmm + 8 * lmm + 1) as usize;
            let wa = vect(wa_len as c_int);
            let iwa_len = (3 * npar) as usize;
            let iwa = vect_int(iwa_len as c_int);
            let g = vect(npar);
            let mut task: [c_char; 60] = [0; 60];
            let mut pgtol_val = pgtol;
            let mut isave: [c_int; 44] = [0; 44];
            let mut lmsg: [u8; 60] = [0; 60];

            // Initialize task to "START"
            let start_str = b"START\0";
            for (i, &b) in start_str.iter().enumerate() {
                task[i] = b as c_char;
            }

            // Stateful loop: lbfgsb returns with task indicating what to do next
            loop {
                lbfgsb(
                    npar,
                    lmm,
                    dpar,
                    lower,
                    upper,
                    nbd,
                    &mut val,
                    g,
                    factr,
                    &mut pgtol_val,
                    wa,
                    iwa,
                    task.as_mut_ptr(),
                    trace,
                    isave.as_mut_ptr(),
                );

                // Read task as bytes
                let task_len = task.iter().position(|&c| c == 0).unwrap_or(60);
                let task_bytes: &[u8] =
                    unsafe { std::slice::from_raw_parts(task.as_ptr() as *const u8, task_len) };

                // Check for convergence
                if task_bytes.starts_with(b"CONVERGENCE") {
                    ifail = 0;
                    for j in 0..task_len.min(60) {
                        lmsg[j] = task_bytes[j];
                    }
                    fncount = isave[33]; // nfgv (function/gradient count)
                    grcount = fncount;
                    break;
                }

                // Check for error
                if task_bytes.starts_with(b"ERROR") {
                    ifail = 1;
                    for j in 0..task_len.min(60) {
                        lmsg[j] = task_bytes[j];
                    }
                    fncount = isave[33];
                    grcount = fncount;
                    break;
                }

                // FG_START or FG or NEW_X or FG_LN: compute function value and gradient
                fncount += 1;
                val = fminfn(npar, dpar, os_ptr as *mut std::ffi::c_void);
                grcount += 1;
                fmingr(npar, dpar, g, os_ptr as *mut std::ffi::c_void);

                // Check iteration limit (isave[29] = iter)
                if isave[29] >= maxit {
                    ifail = 51; // iteration limit reached
                    for j in 0..task_len.min(60) {
                        lmsg[j] = task_bytes[j];
                    }
                    break;
                }
            }

            for i in 0..npar {
                *REAL(par).add(i as usize) =
                    *dpar.add(i as usize) * *(*os_ptr).parscale.add(i as usize);
            }
            let msg_len = lmsg.iter().position(|&c| c == 0).unwrap_or(60);
            let msg_str = String::from_utf8_lossy(&lmsg[..msg_len]);
            let smsg = Rf_mkString(CString::new(msg_str.as_ref()).unwrap_or_default().as_ptr());
            let _smsg_guard = protect(smsg);
            SET_VECTOR_ELT(res, 4, smsg);
        } else {
            Rf_error(b"unknown 'method'\0".as_ptr() as *const _);
        }

        if Rf_isNull((*os_ptr).names) == 0 {
            setAttrib(par, R_NamesSymbol(), (*os_ptr).names);
        }
        *REAL(value).add(0) = val * (*os_ptr).fnscale;
        SET_VECTOR_ELT(res, 0, par);
        SET_VECTOR_ELT(res, 1, value);
        *INTEGER(counts).add(0) = fncount;
        *INTEGER(counts).add(1) = grcount;
        SET_VECTOR_ELT(res, 2, counts);
        *INTEGER(conv).add(0) = ifail;
        SET_VECTOR_ELT(res, 3, conv);
        res
    }
}

// ---------------------------------------------------------------------------
// optimhess -- numerical Hessian computation
// ---------------------------------------------------------------------------

pub unsafe fn optimhess(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut args = CDR(args);
        let mut os = Box::new(OptStruct {
            R_fcall: ptr::null_mut(),
            R_gcall: ptr::null_mut(),
            R_env: rho,
            ndeps: ptr::null_mut(),
            fnscale: 1.0,
            parscale: ptr::null_mut(),
            usebounds: 0,
            lower: ptr::null_mut(),
            upper: ptr::null_mut(),
            names: R_NilValue(),
        });
        let os_ptr: *mut OptStruct = &mut *os;

        (*os_ptr).usebounds = 0;
        (*os_ptr).R_env = rho;

        let par = CAR(args);
        let npar = LENGTH(par);
        (*os_ptr).names = getAttrib(par, R_NamesSymbol());
        args = CDR(args);
        let fn_sexp = CAR(args);
        if !is_function(fn_sexp) {
            Rf_error(b"'fn' is not a function\0".as_ptr() as *const _);
        }
        args = CDR(args);
        let gr = CAR(args);
        args = CDR(args);
        let options = CAR(args);

        (*os_ptr).fnscale = as_real(getListElement(
            options,
            CString::new("fnscale").unwrap_or_default().as_ptr(),
        ));
        let tmp = getListElement(
            options,
            CString::new("parscale").unwrap_or_default().as_ptr(),
        );
        if LENGTH(tmp) != npar {
            Rf_error(b"'parscale' is of the wrong length\0".as_ptr() as *const _);
        }
        (*os_ptr).parscale = vect(npar);
        {
            let tmp = coerceVector(tmp, SEXPTYPE::REALSXP.as_c_int());
            let _tmp_guard = protect(tmp);
            for i in 0..npar {
                *(*os_ptr).parscale.add(i as usize) = *REAL(tmp).add(i as usize);
            }
        }

        (*os_ptr).R_fcall = Rf_lang2(fn_sexp, R_NilValue());
        let _fcall_guard = protect((*os_ptr).R_fcall);

        let par = coerceVector(par, SEXPTYPE::REALSXP.as_c_int());
        let _par_guard = protect(par);

        if Rf_isNull(gr) == 0 {
            if !is_function(gr) {
                Rf_error(b"'gr' is not a function\0".as_ptr() as *const _);
            }
            (*os_ptr).R_gcall = Rf_lang2(gr, R_NilValue());
        } else {
            (*os_ptr).R_gcall = R_NilValue();
        }
        let _gcall_guard = protect((*os_ptr).R_gcall);

        let ndeps = getListElement(options, CString::new("ndeps").unwrap_or_default().as_ptr());
        if LENGTH(ndeps) != npar {
            Rf_error(b"'ndeps' is of the wrong length\0".as_ptr() as *const _);
        }
        (*os_ptr).ndeps = vect(npar);
        {
            let ndeps = coerceVector(ndeps, SEXPTYPE::REALSXP.as_c_int());
            let _ndeps_guard = protect(ndeps);
            for i in 0..npar {
                *(*os_ptr).ndeps.add(i as usize) = *REAL(ndeps).add(i as usize);
            }
        }

        let ans = allocMatrix(SEXPTYPE::REALSXP.into(), npar, npar);
        let _ans_guard = protect(ans);
        let dpar = vect(npar);
        for i in 0..npar {
            *dpar.add(i as usize) =
                *REAL(par).add(i as usize) / *(*os_ptr).parscale.add(i as usize);
        }
        let df1 = vect(npar);
        let df2 = vect(npar);

        for i in 0..npar {
            let eps = *(*os_ptr).ndeps.add(i as usize) / *(*os_ptr).parscale.add(i as usize);
            *dpar.add(i as usize) += eps;
            fmingr(npar, dpar, df1, os_ptr as *mut std::ffi::c_void);
            *dpar.add(i as usize) -= 2.0 * eps;
            fmingr(npar, dpar, df2, os_ptr as *mut std::ffi::c_void);
            for j in 0..npar {
                *REAL(ans).add((i * npar + j) as usize) = (*os_ptr).fnscale
                    * (*df1.add(j as usize) - *df2.add(j as usize))
                    / (2.0
                        * eps
                        * *(*os_ptr).parscale.add(i as usize)
                        * *(*os_ptr).parscale.add(j as usize));
            }
            *dpar.add(i as usize) += eps;
        }

        // Symmetrize
        for i in 0..npar {
            for j in 0..i {
                let tmp_val = 0.5
                    * (*REAL(ans).add((i * npar + j) as usize)
                        + *REAL(ans).add((j * npar + i) as usize));
                *REAL(ans).add((i * npar + j) as usize) = tmp_val;
                *REAL(ans).add((j * npar + i) as usize) = tmp_val;
            }
        }

        let nm = getAttrib(par, R_NamesSymbol());
        if Rf_isNull(nm) == 0 {
            let dm = Rf_allocVector(SEXPTYPE::VECSXP, 2);
            let _dm_guard = protect(dm);
            SET_VECTOR_ELT(dm, 0, duplicate(nm));
            SET_VECTOR_ELT(dm, 1, duplicate(nm));
            setAttrib(ans, R_DimNamesSymbol(), dm);
        }

        ans
    }
}
