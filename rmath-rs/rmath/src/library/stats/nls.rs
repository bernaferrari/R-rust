#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

//! Port of R's src/library/stats/src/nls.c
//!
//! Nonlinear least squares (NLS) iteration and numeric differentiation.
//!
//! Key functions:
//! - nls_iter: Gauss-Newton iteration for NLS model fitting
//! - numeric_deriv: Numerical gradient computation via forward/central differences
//!
//! These functions operate on SEXP objects representing R's nlsModel
//! and nlsControl objects, calling back into R via eval() for model
//! evaluation.

use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_FINITE, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// Helper: R functions — delegate to real implementations
// ---------------------------------------------------------------------------

unsafe fn asInteger(x: SEXP) -> c_int {
    crate::main::coerce::asInteger(x)
}

unsafe fn asLogical(x: SEXP) -> c_int {
    crate::main::coerce::asLogical(x)
}

unsafe fn asReal(x: SEXP) -> c_double {
    crate::main::coerce::asReal(x)
}

unsafe fn asBool(x: SEXP) -> bool {
    let v = asLogical(x);
    v != 0 && v != NA_LOGICAL
}

unsafe fn coerceVector(x: SEXP, sexptype: SEXPTYPE) -> SEXP {
    crate::main::coerce::coerceVector(x, sexptype.0)
}

unsafe fn length(x: SEXP) -> c_int {
    Rf_length(x)
}

unsafe fn allocMatrix(sexptype: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    let ans = Rf_allocVector(sexptype, nrow * ncol);
    Rf_protect(ans);
    let dim = Rf_allocVector(SEXPTYPE::INTSXP.0, 2);
    Rf_protect(dim);
    *INTEGER(dim) = nrow;
    *INTEGER(dim.add(1)) = ncol;
    setAttrib(ans, R_DimSymbol(), dim);
    Rf_unprotect(2);
    ans
}

unsafe fn setAttrib(x: SEXP, what: SEXP, value: SEXP) {
    crate::attrib_core::setAttrib(x, what, value);
}

unsafe fn getAttrib(x: SEXP, what: SEXP) -> SEXP {
    crate::attrib_core::getAttrib(x, what)
}

unsafe fn R_DimSymbol() -> SEXP {
    crate::attrib_core::R_DimSymbol()
}

unsafe fn R_NamesSymbol() -> SEXP {
    crate::attrib_core::R_NamesSymbol()
}

unsafe fn R_ClassSymbol() -> SEXP {
    crate::attrib_core::R_ClassSymbol()
}

unsafe fn R_GlobalEnv() -> SEXP {
    crate::sexp::globals::R_GlobalEnv()
}

unsafe fn R_BaseEnv() -> SEXP {
    crate::sexp::globals::R_BaseEnv()
}

unsafe fn R_NewEnv(enclos: SEXP, hash: bool, size: c_int) -> SEXP {
    crate::sexp::envir::R_NewEnv(enclos, if hash { 1 } else { 0 }, size)
}

unsafe fn duplicate(x: SEXP) -> SEXP {
    crate::main::duplicate::duplicate(x)
}

unsafe fn findVar(sym: SEXP, rho: SEXP) -> SEXP {
    crate::sexp::envir::findVar(sym, rho)
}

#[unsafe(no_mangle)]
unsafe fn defineVar(sym: SEXP, val: SEXP, rho: SEXP) {
    crate::sexp::envir::defineVar(sym, val, rho)
}

unsafe fn install(name: &str) -> SEXP {
    let c_name = std::ffi::CString::new(name).unwrap_or_default();
    crate::sexp::symbol::Rf_install(c_name.as_ptr())
}

unsafe fn eval(expr: SEXP, rho: SEXP) -> SEXP {
    crate::eval::eval::Rf_eval(expr, rho)
}

unsafe fn isNull(x: SEXP) -> bool {
    Rf_isNull(x) != 0
}

unsafe fn isNewList(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::VECSXP.0
}

unsafe fn isFunction(x: SEXP) -> bool {
    let t = TYPEOF(x);
    t == SEXPTYPE::CLOSXP.0 || t == SEXPTYPE::BUILTINSXP.0 || t == SEXPTYPE::SPECIALSXP.0
}

unsafe fn isNumeric(x: SEXP) -> bool {
    crate::main::coerce::isNumeric(x)
}

unsafe fn isLogical(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::LGLSXP.0
}

unsafe fn isReal(x: SEXP) -> bool {
    crate::main::coerce::isReal(x)
}

unsafe fn isInteger(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::INTSXP.0
}

unsafe fn isString(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::STRSXP.0
}

unsafe fn isEnvironment(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::ENVSXP.0
}

unsafe fn lang1(fn_: SEXP) -> SEXP {
    // Create a one-element call: CONS(fn_, R_NilValue())
    crate::sexp::constructors::Rf_cons(fn_, R_NilValue())
}

unsafe fn lang2(fn_: SEXP, arg: SEXP) -> SEXP {
    crate::sexp::constructors::Rf_lang2(fn_, arg)
}

unsafe fn mkNamed(sexptype: c_int, names: &[&str]) -> SEXP {
    let n = names.len();
    let ans = Rf_protect(Rf_allocVector(sexptype, n as c_int));
    let nms = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, n as c_int));
    for i in 0..n {
        if !names[i].is_empty() {
            SET_STRING_ELT(
                nms,
                i as i64,
                crate::sexp::constructors::Rf_mkChar(
                    std::ffi::CString::new(names[i])
                        .unwrap_or_default()
                        .as_ptr(),
                ),
            );
        }
    }
    setAttrib(ans, R_NamesSymbol(), nms);
    Rf_unprotect(2);
    ans
}

#[unsafe(no_mangle)]
unsafe fn mkString(s: &str) -> SEXP {
    let c_s = std::ffi::CString::new(s).unwrap_or_default();
    crate::sexp::constructors::Rf_mkString(c_s.as_ptr())
}

unsafe fn translateChar(x: SEXP) -> *const c_char {
    crate::sexp::accessors::translateChar(x)
}

unsafe fn MARK_NOT_MUTABLE(x: SEXP) {
    if !x.is_null() {
        (*x).sxpinfo.set_named(2);
    }
}

unsafe fn Rprintf(msg: &str) {
    eprint!("{}", msg);
}

unsafe fn error(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    crate::main::errors::Rf_error(c_msg.as_ptr());
}

unsafe fn warning(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    crate::main::errors::Rf_warning(c_msg.as_ptr());
}

#[inline]
fn MIN(a: c_int, b: c_int) -> c_int {
    if a < b { a } else { b }
}

// ---------------------------------------------------------------------------
// getListElement: find a named element in a list
// ---------------------------------------------------------------------------

unsafe fn getListElement(list: SEXP, names: SEXP, str: &str) -> SEXP {
    let len = length(list);
    for i in 0..len as usize {
        let temp_char = CHAR(STRING_ELT(names, i as i64));
        if !temp_char.is_null() {
            let c_str = std::ffi::CStr::from_ptr(temp_char);
            if c_str.to_str().map_or(false, |s| s == str) {
                return VECTOR_ELT(list, i as i64);
            }
        }
    }
    ptr::null_mut()
}

// ---------------------------------------------------------------------------
// ConvInfoMsg: build convergence info list
// ---------------------------------------------------------------------------

unsafe fn ConvInfoMsg(
    msg: &str,
    iter: c_int,
    whystop: c_int,
    _fac: f64,
    _min_fac: f64,
    _max_iter: c_int,
    conv_new: f64,
) -> SEXP {
    let nms = ["isConv", "finIter", "finTol", "stopCode", "stopMessage", ""];
    let ans = Rf_protect(mkNamed(SEXPTYPE::VECSXP.0, &nms));

    SET_VECTOR_ELT(ans, 0, Rf_ScalarLogical(if whystop == 0 { 1 } else { 0 }));
    SET_VECTOR_ELT(ans, 1, Rf_ScalarInteger(iter));
    SET_VECTOR_ELT(ans, 2, Rf_ScalarReal(conv_new));
    SET_VECTOR_ELT(ans, 3, Rf_ScalarInteger(whystop));
    SET_VECTOR_ELT(ans, 4, mkString(msg));

    Rf_unprotect(1);
    ans
}

// ---------------------------------------------------------------------------
// nls_iter: Gauss-Newton iteration for NLS
// ---------------------------------------------------------------------------

/// R entry point: nls_iter(m, control, doTrace)
///
/// Performs Gauss-Newton iteration for nonlinear least squares.
/// `m` is an nlsModel object, `control` is an nlsControl object.
pub unsafe fn nls_iter(m: SEXP, control: SEXP, doTraceArg: SEXP) -> SEXP {
    let doTrace = asLogical(doTraceArg) != 0;

    if !isNewList(control) {
        error("'control' must be a list");
    }
    if !isNewList(m) {
        error("'m' must be a list");
    }

    let mut tmp = Rf_protect(getAttrib(control, R_NamesSymbol()));

    let mut conv = getListElement(control, tmp, "maxiter");
    if conv.is_null() || !isNumeric(conv) {
        error("'%s' absent", "control$maxiter");
    }
    let maxIter = asInteger(conv);

    conv = getListElement(control, tmp, "tol");
    if conv.is_null() || !isNumeric(conv) {
        error("'%s' absent", "control$tol");
    }
    let tolerance = asReal(conv);

    conv = getListElement(control, tmp, "minFactor");
    if conv.is_null() || !isNumeric(conv) {
        error("'%s' absent", "control$minFactor");
    }
    let minFac = asReal(conv);

    conv = getListElement(control, tmp, "warnOnly");
    if conv.is_null() || !isLogical(conv) {
        error("'%s' absent", "control$warnOnly");
    }
    let warnOnly = asLogical(conv) != 0;

    conv = getListElement(control, tmp, "printEval");
    if conv.is_null() || !isLogical(conv) {
        error("'%s' absent", "control$printEval");
    }
    let printEval = asBool(conv);

    // Get parts from 'm'
    tmp = getAttrib(m, R_NamesSymbol());

    conv = getListElement(m, tmp, "conv");
    if conv.is_null() || !isFunction(conv) {
        error("'%s' absent", "m$conv()");
    }
    let conv_call = Rf_protect(lang1(conv));

    let incr = getListElement(m, tmp, "incr");
    if incr.is_null() || !isFunction(incr) {
        error("'%s' absent", "m$incr()");
    }
    let incr_call = Rf_protect(lang1(incr));

    let deviance = getListElement(m, tmp, "deviance");
    if deviance.is_null() || !isFunction(deviance) {
        error("'%s' absent", "m$deviance()");
    }
    let deviance_call = Rf_protect(lang1(deviance));

    let trace_fn = getListElement(m, tmp, "trace");
    if trace_fn.is_null() || !isFunction(trace_fn) {
        error("'%s' absent", "m$trace()");
    }
    let trace_call = Rf_protect(lang1(trace_fn));

    let setPars = getListElement(m, tmp, "setPars");
    if setPars.is_null() || !isFunction(setPars) {
        error("'%s' absent", "m$setPars()");
    }
    Rf_protect(setPars);

    let getPars = getListElement(m, tmp, "getPars");
    if getPars.is_null() || !isFunction(getPars) {
        error("'%s' absent", "m$getPars()");
    }
    let getPars_call = Rf_protect(lang1(getPars));

    let pars = Rf_protect(eval(getPars_call, R_GlobalEnv()));
    let nPars = LENGTH(pars);

    let mut dev = asReal(eval(deviance_call, R_GlobalEnv()));
    if doTrace {
        eval(trace_call, R_GlobalEnv());
    }

    let mut fac: f64 = 1.0;
    let mut hasConverged = false;
    let newPars = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, nPars));
    let mut evaltotCnt: c_int = 1;
    let mut convNew: f64 = -1.0;
    let mut i: c_int;

    // Main iteration loop
    for iter_i in 0..maxIter {
        i = iter_i;

        let conv_val = asReal(eval(conv_call, R_GlobalEnv()));
        convNew = conv_val;
        if conv_val <= tolerance {
            hasConverged = true;
            break;
        }

        let newIncr = Rf_protect(eval(incr_call, R_GlobalEnv()));
        let par = REAL(pars);
        let npar = REAL(newPars);
        let nIncr = REAL(newIncr);
        let mut evalCnt: c_int = -1;
        if printEval {
            evalCnt = 1;
        }

        // Line search
        while fac >= minFac {
            if printEval {
                Rprintf(&format!(
                    "  It. {:3}, fac= {:11.6}, eval (no.,total): ({:2},{:3}):",
                    i + 1,
                    fac,
                    evalCnt,
                    evaltotCnt
                ));
                evalCnt += 1;
                evaltotCnt += 1;
            }
            for j in 0..nPars as usize {
                *npar.add(j) = *par.add(j) + fac * *nIncr.add(j);
            }

            tmp = lang2(setPars, newPars);
            let set_result = asLogical(eval(tmp, R_GlobalEnv()));
            if set_result != 0 {
                // Singular gradient
                Rf_unprotect(11);
                if warnOnly {
                    warning("singular gradient");
                    return ConvInfoMsg("singular gradient", i, 1, fac, minFac, maxIter, convNew);
                } else {
                    error("singular gradient");
                }
            }

            let newDev = asReal(eval(deviance_call, R_GlobalEnv()));
            if printEval {
                Rprintf(&format!(" new dev = {}\n", newDev));
            }
            if newDev <= dev {
                dev = newDev;
                // Swap pars and newPars
                let pars_ptr = pars;
                let newPars_ptr = newPars;
                // Note: in C this swaps the SEXP pointers.
                // In Rust we can't easily swap raw pointers, so we copy the data.
                for j in 0..nPars as usize {
                    let tmp_val = *REAL(pars_ptr).add(j);
                    *REAL(pars_ptr).add(j) = *REAL(newPars_ptr).add(j);
                    *REAL(newPars_ptr).add(j) = tmp_val;
                }
                fac = MIN(2.0 as c_int * fac as c_int, 1) as f64;
                break;
            }
            fac /= 2.0;
        }
        Rf_unprotect(1);
        if doTrace {
            eval(trace_call, R_GlobalEnv());
        }
        if fac < minFac {
            Rf_unprotect(9);
            if warnOnly {
                let msg = format!(
                    "step factor {} reduced below 'minFactor' of {}",
                    fac, minFac
                );
                warning(&msg);
                return ConvInfoMsg(&msg, i, 2, fac, minFac, maxIter, convNew);
            } else {
                error("step factor reduced below 'minFactor'");
            }
        }
    }

    Rf_unprotect(9);
    if !hasConverged {
        if warnOnly {
            let msg = format!("number of iterations exceeded maximum of {}", maxIter);
            warning(&msg);
            return ConvInfoMsg(&msg, i, 3, fac, minFac, maxIter, convNew);
        } else {
            error("number of iterations exceeded maximum");
        }
    } else {
        ConvInfoMsg("converged", i, 0, fac, minFac, maxIter, convNew)
    }
}

// ---------------------------------------------------------------------------
// numeric_deriv: Numerical gradient computation
// ---------------------------------------------------------------------------

/// R entry point: numeric_deriv(expr, theta, rho, dir, eps, centr)
///
/// Computes the numerical gradient of `expr` with respect to variables
/// named in `theta`, evaluated in environment `rho`.
/// Uses forward differences (default) or central differences.
pub unsafe fn numeric_deriv(
    expr: SEXP,
    theta: SEXP,
    mut rho: SEXP,
    mut dir: SEXP,
    eps_: SEXP,
    centr: SEXP,
) -> SEXP {
    if !isString(theta) {
        error("'theta' should be of type character");
    }
    if isNull(rho) {
        error("use of NULL environment is defunct");
    } else if !isEnvironment(rho) {
        error("'rho' should be an environment");
    }

    let mut nprot: c_int = 3;
    if TYPEOF(dir) != SEXPTYPE::REALSXP.0 {
        dir = Rf_protect(coerceVector(dir, SEXPTYPE::REALSXP));
        nprot += 1;
    }
    if LENGTH(dir) != LENGTH(theta) {
        error("'dir' is not a numeric vector of the correct length");
    }

    let central = asBool(centr);
    if asLogical(centr) == NA_LOGICAL {
        error("'central' is NA, but must be TRUE or FALSE");
    }

    let rho1 = Rf_protect(R_NewEnv(rho, false, 0));
    nprot += 1;

    let pars = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, LENGTH(theta)));
    let mut ans = Rf_protect(duplicate(eval(expr, rho1)));
    let rDir = REAL(dir);
    let mut res: *mut c_double = ptr::null_mut();

    // CHECK_FN_VAL macro equivalent
    unsafe fn check_fn_val<'a>(r: &mut *mut c_double, ans_ref: &mut SEXP) {
        if !isReal(*ans_ref) {
            let temp = coerceVector(*ans_ref, SEXPTYPE::REALSXP);
            Rf_unprotect(1);
            *ans_ref = Rf_protect(temp);
        }
        *r = REAL(*ans_ref);
        for i in 0..LENGTH(*ans_ref) as usize {
            if !R_FINITE(*(*r).add(i)) {
                error("Missing value or an infinity produced when evaluating the model");
            }
        }
    }

    check_fn_val(&mut res, &mut ans);

    let mut lengthTheta: c_int = 0;
    for i in 0..LENGTH(theta) as usize {
        let name_ptr = translateChar(STRING_ELT(theta, i as i64));
        if name_ptr.is_null() {
            continue;
        }
        let name = std::ffi::CStr::from_ptr(name_ptr).to_str().unwrap_or("");
        let s_name = install(name);
        let temp = findVar(s_name, rho1);
        if isInteger(temp) {
            error("variable '%s' is integer, not numeric", name);
        }
        if !isReal(temp) {
            error("variable '%s' is not numeric", name);
        }
        // Make a copy since we'll be modifying the variable
        let temp_copy = duplicate(temp);
        defineVar(s_name, temp_copy, rho1);
        MARK_NOT_MUTABLE(temp_copy);
        SET_VECTOR_ELT(pars, i as i64, temp_copy);
        lengthTheta += LENGTH(VECTOR_ELT(pars, i as i64));
    }

    let gradient = Rf_protect(allocMatrix(SEXPTYPE::REALSXP.0, LENGTH(ans), lengthTheta));
    let grad = REAL(gradient);
    let eps = asReal(eps_);

    let mut start: c_int = 0;
    for i in 0..LENGTH(theta) as usize {
        let pars_i = REAL(VECTOR_ELT(pars, i as i64));
        for j in 0..LENGTH(VECTOR_ELT(pars, i as i64)) as usize {
            let origPar = *pars_i.add(j);
            let xx = origPar.abs();
            let delta = if xx == 0.0 { eps } else { xx * eps };

            *pars_i.add(j) = origPar + *rDir.add(i) * delta;
            let ans_del = Rf_protect(eval(expr, rho1));
            let mut rDel: *mut c_double = ptr::null_mut();
            check_fn_val(&mut rDel, &mut ans_del);

            if central {
                *pars_i.add(j) = origPar - *rDir.add(i) * delta;
                let ans_de2 = Rf_protect(eval(expr, rho1));
                let mut rD2: *mut c_double = ptr::null_mut();
                check_fn_val(&mut rD2, &mut ans_de2);

                for k in 0..LENGTH(ans) as usize {
                    *grad.add((start + k) as usize) =
                        *rDir.add(i) * (*rDel.add(k) - *rD2.add(k)) / (2.0 * delta);
                }
                Rf_unprotect(2); // ans_de2, ans_del
            } else {
                for k in 0..LENGTH(ans) as usize {
                    *grad.add((start + k) as usize) =
                        *rDir.add(i) * (*rDel.add(k) - *res.add(k)) / delta;
                }
                Rf_unprotect(1); // ans_del
            }

            *pars_i.add(j) = origPar;
            start += LENGTH(ans);
        }
    }

    setAttrib(ans, install("gradient"), gradient);
    Rf_unprotect(nprot);
    ans
}
