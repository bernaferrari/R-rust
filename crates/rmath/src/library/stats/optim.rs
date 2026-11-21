/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1999-2025  The R Core Team
 *
 *  Ported to Rust from r-source/src/library/stats/src/optim.c
 */

use std::ffi::CString;
use std::os::raw::{c_char, c_double, c_int};

use super::checked::{char_bytes, duplicate, eval, mk_char, mk_string, rprintf};
use crate::main::errors::{Rf_error, Rf_error_cstr};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::memory_ext::{R_alloc, TransientScope};
use crate::sexp::owner::StoredOwner;
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// Re-exports of functions defined elsewhere
// ---------------------------------------------------------------------------

use crate::attrib_core::{R_DimNamesSymbol, R_NamesSymbol, getAttrib, setAttrib};

// ---------------------------------------------------------------------------
// Local SETCADR helper
// ---------------------------------------------------------------------------

fn SETCADR(x: SEXP, y: SEXP) {
    if !x.is_null() {
        let cdr = CDR(x);
        if !cdr.is_null() {
            SETCAR(cdr, y);
        }
    }
}

/// The caller retains this scope across every optimizer callback. The mark
/// releases all scratch arrays on success and R error or interrupt unwind.
fn optimization_scope<'session>() -> TransientScope<'session> {
    let owner = super::checked::current_owner();
    TransientScope::enter(StoredOwner::from_token(owner))
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Element types whose all-zero bit pattern is a valid value.
trait ZeroValid: Copy {}
impl ZeroValid for c_double {}
impl ZeroValid for c_int {}

/// Budgeted transient scratch (GNU `vect`): `R_alloc` storage charged to the
/// session's transient memory budget, released when `scope` drops.
fn vect<'s, T: ZeroValid>(_scope: &'s TransientScope<'_>, n: usize) -> &'s mut [T] {
    if n == 0 {
        return &mut [];
    }
    // SAFETY: R_alloc returns a fresh, zero-filled, u64-aligned buffer of
    // `n * size_of::<T>()` bytes (or raises an R error), and every `T` is
    // valid when zeroed. The buffer is owned by the transient stack above
    // `scope`'s mark, so it stays live and unaliased until `scope` drops,
    // which the `'s` borrow prevents while the slice exists.
    unsafe { std::slice::from_raw_parts_mut(R_alloc(size_of::<T>(), n).cast::<T>(), n) }
}

// ---------------------------------------------------------------------------
// OptStruct -- optimization state
// ---------------------------------------------------------------------------

struct OptStruct<'s> {
    R_fcall: SEXP,
    R_gcall: SEXP,
    R_env: SEXP,
    ndeps: &'s [c_double],
    fnscale: c_double,
    parscale: &'s [c_double],
    usebounds: c_int,
    lower: &'s [c_double],
    upper: &'s [c_double],
    names: SEXP,
}

impl<'s> OptStruct<'s> {
    fn new(rho: SEXP) -> Self {
        OptStruct {
            R_fcall: std::ptr::null_mut(),
            R_gcall: std::ptr::null_mut(),
            R_env: rho,
            ndeps: &[],
            fnscale: 1.0,
            parscale: &[],
            usebounds: 0,
            lower: &[],
            upper: &[],
            names: R_NilValue(),
        }
    }

    /// Opaque `ex` pointer handed to the appl kernels and back to the
    /// `fminfn`/`fmingr` trampolines.
    fn as_ex(&mut self) -> *mut std::ffi::c_void {
        (self as *mut OptStruct<'s>).cast()
    }
}

// ---------------------------------------------------------------------------
// getListElement -- get named element from a list
// ---------------------------------------------------------------------------

fn getListElement(list: SEXP, name: &str) -> SEXP {
    if TYPEOF(list) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }
    let names = getAttrib(list, R_NamesSymbol());
    if TYPEOF(names) != SEXPTYPE::STRSXP {
        return R_NilValue();
    }
    let target = name.as_bytes();
    let len = LENGTH(list).min(LENGTH(names));
    for i in 0..len {
        let name_sexp = STRING_ELT(names, i as R_xlen_t);
        if name_sexp.is_null() {
            continue;
        }
        if char_bytes(name_sexp) == target {
            return VECTOR_ELT(list, i as R_xlen_t);
        }
    }
    R_NilValue()
}

/// Copy a list option coerced to double into fresh scratch, as GNU's
/// `vect(npar)` filled from `REAL(coerceVector(..))`.
fn real_option<'s>(scope: &'s TransientScope<'_>, x: SEXP, npar: c_int) -> &'s [c_double] {
    let out = vect(scope, npar.max(0) as usize);
    let x = coerceVector(x, SEXPTYPE::REALSXP.as_c_int());
    let _guard = protect(x);
    for (i, v) in out.iter_mut().enumerate() {
        *v = REAL_ELT(x, i as c_int);
    }
    out
}

fn non_finite_par() -> ! {
    let msg = CString::new("non-finite value supplied by optim").unwrap_or_default();
    Rf_error_cstr(&msg);
    unreachable!("Rf_error returned")
}

/// Fill a fresh REALSXP with `p * parscale`, rejecting non-finite input.
fn scaled_par(os: &OptStruct<'_>, p: &[c_double], check_finite: bool, names: bool) -> SEXP {
    let n = p.len() as c_int;
    let x = Rf_allocVector(SEXPTYPE::REALSXP, n);
    let _x_guard = protect(x);
    if names {
        setAttrib(x, R_NamesSymbol(), os.names);
    }
    for i in 0..p.len() {
        if check_finite && !R_FINITE(p[i]) {
            non_finite_par();
        }
        SET_REAL_ELT(x, i as c_int, p[i] * os.parscale[i]);
    }
    x
}

/// Evaluate `call` and return its first element coerced to double.
fn eval_first_real(call: SEXP, env: SEXP) -> c_double {
    let s = eval(call, env);
    let _s_guard = protect(s);
    let s = coerceVector(s, SEXPTYPE::REALSXP.as_c_int());
    let _coerced_guard = protect(s);
    REAL_ELT(s, 0)
}

// ---------------------------------------------------------------------------
// objective / gradient -- checked bodies of the optim callbacks
// ---------------------------------------------------------------------------

fn objective(os: &OptStruct<'_>, p: &[c_double]) -> c_double {
    let x = scaled_par(os, p, true, Rf_isNull(os.names) == 0);
    let _x_guard = protect(x);
    SETCADR(os.R_fcall, x);
    let s = eval(os.R_fcall, os.R_env);
    let _s_guard = protect(s);
    let s = coerceVector(s, SEXPTYPE::REALSXP.as_c_int());
    let _coerced_guard = protect(s);
    if LENGTH(s) != 1 {
        let msg = CString::new(format!(
            "objective function in optim evaluates to length {} not 1",
            LENGTH(s)
        ))
        .unwrap_or_default();
        Rf_error_cstr(&msg);
    }
    REAL_ELT(s, 0) / os.fnscale
}

fn gradient(os: &OptStruct<'_>, p: &[c_double], df: &mut [c_double]) {
    let n = p.len() as c_int;
    if Rf_isNull(os.R_gcall) == 0 {
        // Analytical derivatives
        let x = scaled_par(os, p, true, Rf_isNull(os.names) == 0);
        let _x_guard = protect(x);
        SETCADR(os.R_gcall, x);
        let s = eval(os.R_gcall, os.R_env);
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
            Rf_error_cstr(&msg);
        }
        for i in 0..p.len() {
            df[i] = REAL_ELT(s, i as c_int) * os.parscale[i] / os.fnscale;
        }
    } else {
        // Numerical derivatives
        let x = scaled_par(os, p, false, true);
        let _x_guard = protect(x);
        SETCADR(os.R_fcall, x);

        for i in 0..p.len() {
            let ic = i as c_int;
            let (lo, hi, denom);
            if os.usebounds == 0 {
                let eps = os.ndeps[i];
                hi = p[i] + eps;
                lo = p[i] - eps;
                denom = 2.0 * eps;
            } else {
                let mut epsused = os.ndeps[i];
                let mut eps = epsused;
                let mut tmp = p[i] + eps;
                if tmp > os.upper[i] {
                    tmp = os.upper[i];
                    epsused = tmp - p[i];
                }
                hi = tmp;
                tmp = p[i] - eps;
                if tmp < os.lower[i] {
                    tmp = os.lower[i];
                    eps = p[i] - tmp;
                }
                lo = tmp;
                denom = epsused + eps;
            }
            SET_REAL_ELT(x, ic, hi * os.parscale[i]);
            let val1 = eval_first_real(os.R_fcall, os.R_env) / os.fnscale;
            SET_REAL_ELT(x, ic, lo * os.parscale[i]);
            let val2 = eval_first_real(os.R_fcall, os.R_env) / os.fnscale;
            df[i] = (val1 - val2) / denom;
            if !R_FINITE(df[i]) {
                let msg = CString::new(format!("non-finite finite-difference value [{}]", i + 1))
                    .unwrap_or_default();
                Rf_error_cstr(&msg);
            }
            SET_REAL_ELT(x, ic, p[i] * os.parscale[i]);
        }
    }
}

// ---------------------------------------------------------------------------
// fminfn / fmingr -- ABI trampolines registered with the appl kernels
// ---------------------------------------------------------------------------

unsafe extern "C-unwind" fn fminfn(
    n: c_int,
    p: *mut c_double,
    ex: *mut std::ffi::c_void,
) -> c_double {
    // SAFETY: the appl kernels pass back the `OptStruct` installed by
    // `OptStruct::as_ex` (only read here) and a parameter buffer holding `n`
    // initialized doubles that stays live and unaliased for this call.
    let (os, p) = unsafe {
        (
            &*(ex as *const OptStruct<'_>),
            std::slice::from_raw_parts(p, n.max(0) as usize),
        )
    };
    objective(os, p)
}

unsafe extern "C-unwind" fn fmingr(
    n: c_int,
    p: *mut c_double,
    df: *mut c_double,
    ex: *mut std::ffi::c_void,
) {
    // SAFETY: as for `fminfn`; additionally `df` is a distinct writable
    // buffer of `n` doubles owned by the kernel for this call.
    let (os, p, df) = unsafe {
        (
            &*(ex as *const OptStruct<'_>),
            std::slice::from_raw_parts(p, n.max(0) as usize),
            std::slice::from_raw_parts_mut(df, n.max(0) as usize),
        )
    };
    gradient(os, p, df)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn as_real(x: SEXP) -> c_double {
    crate::mainutils::coerce::asReal(x)
}

fn as_integer(x: SEXP) -> c_int {
    crate::mainutils::coerce::asInteger(x)
}

fn is_function(x: SEXP) -> bool {
    if x.is_null() {
        return false;
    }
    let t = TYPEOF(x);
    t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP
}

fn is_string(x: SEXP) -> bool {
    if x.is_null() {
        return false;
    }
    TYPEOF(x) == SEXPTYPE::STRSXP
}

fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::main::coerce::coerceVector(x, type_)
}

fn allocMatrix(sexptype: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    let ans = Rf_allocVector(sexptype, nrow * ncol);
    let _ans_guard = protect(ans);
    let dim = Rf_allocVector(SEXPTYPE::INTSXP, 2);
    let _dim_guard = protect(dim);
    SET_INTEGER_ELT(dim, 0, nrow);
    SET_INTEGER_ELT(dim, 1, ncol);
    crate::attrib_core::setAttrib(ans, crate::attrib_core::R_DimSymbol(), dim);
    ans
}

use crate::appl::lbfgsb::{LbfgsbState, lbfgsb, workspace_lengths};
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

fn genptry(x: &[f64], xp: &mut [f64], t: f64, os: &OptStruct<'_>) -> f64 {
    const E1: f64 = 1.7182818; /* exp(1) - 1 */

    for i in 0..x.len() {
        /* generate a Cauchy variate */
        let u1 = norm_rand();
        let u2 = norm_rand();
        let cauchy = u1 / u2;

        /* scale by temperature and current value */
        let mut s = x[i] + t * cauchy * (1.0 + E1 * x[i].abs());
        /* check that the new point is finite */
        if !R_FINITE(s) {
            s = x[i];
        }
        xp[i] = s;
    }

    objective(os, xp)
}

// ---------------------------------------------------------------------------
// samin -- simulated annealing optimization (ported from R source:
//           src/appl/optim.c lines 720-790)
//
// Simulated annealing using the Metropolis criterion. The function performs
// maxit iterations, each consisting of tmax function evaluations at a given
// temperature. The temperature decreases linearly from temp to near zero.
// ---------------------------------------------------------------------------

fn samin(
    sb: &mut [f64],
    ybest: &mut f64,
    maxit: c_int,
    tmax: c_int,
    temp: f64,
    trace: c_int,
    os: &OptStruct<'_>,
    scope: &TransientScope<'_>,
) {
    const BIG: f64 = 1e30;

    /* Allocate a candidate point */
    let xp: &mut [f64] = vect(scope, sb.len());

    let mut nacc = 0;
    let mut nfcnev = 0;

    /* Evaluate at the initial point */
    *ybest = objective(os, sb);
    nfcnev += 1;

    /* perform the annealing */
    for it in 0..maxit {
        let t = temp * (1.0 - it as f64 / maxit as f64);

        for j in 0..tmax {
            let ytry = genptry(sb, xp, t, os);
            nfcnev += 1;

            /* Metropolis acceptance step */
            let fac = ytry - *ybest;
            if fac < 0.0 {
                /* accept -- new value is better */
                sb.copy_from_slice(xp);
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
                    sb.copy_from_slice(xp);
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

/// Read and validate the `ndeps` option into `os.ndeps`.
fn load_ndeps<'s>(
    os: &mut OptStruct<'s>,
    scope: &'s TransientScope<'_>,
    options: SEXP,
    npar: c_int,
) {
    let ndeps = getListElement(options, "ndeps");
    if LENGTH(ndeps) != npar {
        Rf_error_cstr(c"'ndeps' is of the wrong length");
    }
    os.ndeps = real_option(scope, ndeps, npar);
}

/// Install the gradient call (or R_NilValue) for gradient-using methods.
fn load_gcall(os: &mut OptStruct<'_>, gr: SEXP) {
    if Rf_isNull(gr) == 0 {
        if !is_function(gr) {
            Rf_error_cstr(c"'gr' is not a function");
        }
        os.R_gcall = Rf_lang2(gr, R_NilValue());
    } else {
        os.R_gcall = R_NilValue();
    }
}

/// Store `x * parscale` into the REALSXP `par`.
fn store_par(par: SEXP, x: &[f64], os: &OptStruct<'_>) {
    for i in 0..x.len() {
        SET_REAL_ELT(par, i as c_int, x[i] * os.parscale[i]);
    }
}

// ---------------------------------------------------------------------------
// optim -- main optimization entry point
// ---------------------------------------------------------------------------

pub fn optim(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let scratch = optimization_scope();
    let mut args = CDR(args);
    let mut os = OptStruct::new(rho);

    let par = CAR(args);
    os.names = getAttrib(par, R_NamesSymbol());
    args = CDR(args);
    let fn_sexp = CAR(args);
    if !is_function(fn_sexp) {
        Rf_error_cstr(c"'fn' is not a function");
    }
    args = CDR(args);
    let gr = CAR(args);
    args = CDR(args);
    let method = CAR(args);
    if !is_string(method) || LENGTH(method) != 1 {
        Rf_error_cstr(c"invalid 'method' argument");
    }
    let tn_owned = char_bytes(STRING_ELT(method, 0));
    let tn = tn_owned.as_slice();
    args = CDR(args);
    let options = CAR(args);

    os.R_fcall = Rf_lang2(fn_sexp, R_NilValue());
    let _fcall_guard = protect(os.R_fcall);

    let par = coerceVector(par, SEXPTYPE::REALSXP.as_c_int());
    let _par_guard = protect(par);
    let npar = LENGTH(par);
    let n_usize = npar.max(0) as usize;
    let dpar: &mut [f64] = vect(&scratch, n_usize);
    let opar: &mut [f64] = vect(&scratch, n_usize);

    let trace = as_integer(getListElement(options, "trace"));
    os.fnscale = as_real(getListElement(options, "fnscale"));
    let tmp = getListElement(options, "parscale");
    if LENGTH(tmp) != npar {
        Rf_error_cstr(c"'parscale' is of the wrong length");
    }
    os.parscale = real_option(&scratch, tmp, npar);
    for i in 0..n_usize {
        dpar[i] = REAL_ELT(par, i as c_int) / os.parscale[i];
    }

    let res = Rf_allocVector(SEXPTYPE::VECSXP, 5);
    let _res_guard = protect(res);
    SET_VECTOR_ELT(res, 4, R_NilValue());
    let names = Rf_allocVector(SEXPTYPE::STRSXP, 5);
    let _names_guard = protect(names);
    SET_STRING_ELT(names, 0, mk_char("par"));
    SET_STRING_ELT(names, 1, mk_char("value"));
    SET_STRING_ELT(names, 2, mk_char("counts"));
    SET_STRING_ELT(names, 3, mk_char("convergence"));
    SET_STRING_ELT(names, 4, mk_char("message"));
    setAttrib(res, R_NamesSymbol(), names);

    let value = Rf_allocVector(SEXPTYPE::REALSXP, 1);
    let _value_guard = protect(value);
    let counts = Rf_allocVector(SEXPTYPE::INTSXP, 2);
    let _counts_guard = protect(counts);
    let countnames = Rf_allocVector(SEXPTYPE::STRSXP, 2);
    let _countnames_guard = protect(countnames);
    SET_STRING_ELT(countnames, 0, mk_char("function"));
    SET_STRING_ELT(countnames, 1, mk_char("gradient"));
    setAttrib(counts, R_NamesSymbol(), countnames);

    let conv = Rf_allocVector(SEXPTYPE::INTSXP, 1);
    let _conv_guard = protect(conv);
    let abstol = as_real(getListElement(options, "abstol"));
    let reltol = as_real(getListElement(options, "reltol"));
    let maxit = as_integer(getListElement(options, "maxit"));
    if maxit == NA_INTEGER {
        Rf_error_cstr(c"'maxit' is not an integer");
    }

    let mut fncount: c_int = 0;
    let mut grcount: c_int = 0;
    let mut ifail: c_int = 0;
    let mut val: c_double = 0.0;

    if tn == b"Nelder-Mead" {
        let alpha = as_real(getListElement(options, "alpha"));
        let beta = as_real(getListElement(options, "beta"));
        let gamm = as_real(getListElement(options, "gamma"));
        let ex = os.as_ex();
        // SAFETY: `dpar`/`opar` hold `npar` doubles and `ex` is the live
        // `OptStruct` the `fminfn` trampoline reads; all outlive the call.
        unsafe {
            nmmin(
                npar,
                dpar.as_mut_ptr(),
                opar.as_mut_ptr(),
                &mut val,
                fminfn,
                &mut ifail,
                abstol,
                reltol,
                ex,
                alpha,
                beta,
                gamm,
                trace,
                &mut fncount,
                maxit,
            );
        }
        store_par(par, opar, &os);
        grcount = NA_INTEGER;
    } else if tn == b"SANN" {
        let tmax = as_integer(getListElement(options, "tmax"));
        let temp = as_real(getListElement(options, "temp"));
        let trace_val = if trace != 0 {
            as_integer(getListElement(options, "REPORT"))
        } else {
            0
        };
        if tmax == NA_INTEGER || tmax < 1 {
            Rf_error_cstr(c"'tmax' is not a positive integer");
        }
        load_gcall(&mut os, gr);
        let _gcall_guard = protect(os.R_gcall);
        samin(dpar, &mut val, maxit, tmax, temp, trace_val, &os, &scratch);
        store_par(par, dpar, &os);
        fncount = if npar > 0 { maxit } else { 1 };
        grcount = NA_INTEGER;
    } else if tn == b"BFGS" {
        let nREPORT = as_integer(getListElement(options, "REPORT"));
        load_gcall(&mut os, gr);
        if Rf_isNull(gr) != 0 {
            load_ndeps(&mut os, &scratch, options, npar);
        }
        let _gcall_guard = protect(os.R_gcall);
        let mask: &mut [c_int] = vect(&scratch, n_usize);
        mask.fill(1);
        let ex = os.as_ex();
        // SAFETY: `dpar` and `mask` hold `npar` cells and `ex` is the live
        // `OptStruct` the trampolines read; all outlive the call.
        unsafe {
            vmmin(
                npar,
                dpar.as_mut_ptr(),
                &mut val,
                fminfn,
                fmingr,
                maxit,
                trace,
                mask.as_ptr(),
                abstol,
                reltol,
                nREPORT,
                ex,
                &mut fncount,
                &mut grcount,
                &mut ifail,
            );
        }
        store_par(par, dpar, &os);
    } else if tn == b"CG" {
        let type_val = as_integer(getListElement(options, "type"));
        load_gcall(&mut os, gr);
        if Rf_isNull(gr) != 0 {
            load_ndeps(&mut os, &scratch, options, npar);
        }
        let _gcall_guard = protect(os.R_gcall);
        let ex = os.as_ex();
        // SAFETY: `dpar`/`opar` hold `npar` doubles and `ex` is the live
        // `OptStruct` the trampolines read; all outlive the call.
        unsafe {
            cgmin(
                npar,
                dpar.as_mut_ptr(),
                opar.as_mut_ptr(),
                &mut val,
                fminfn,
                fmingr,
                &mut ifail,
                abstol,
                reltol,
                ex,
                type_val,
                trace,
                &mut fncount,
                &mut grcount,
                maxit,
            );
        }
        store_par(par, opar, &os);
    } else if tn == b"L-BFGS-B" {
        let nREPORT = as_integer(getListElement(options, "REPORT"));
        if nREPORT <= 0 {
            Rf_error_cstr(c"REPORT must be > 0 (method = \"L-BFGS-B\")");
        }
        let factr = as_real(getListElement(options, "factr"));
        let pgtol = as_real(getListElement(options, "pgtol"));
        // Zero invokes GNU's "ERROR: M .LE. 0" result without forming
        // negative pointer offsets for rejected history sizes.
        let lmm = as_integer(getListElement(options, "lmm")).max(0);
        let (wa_len, iwa_len) = workspace_lengths(npar, lmm).unwrap_or_else(|| {
            crate::sexp::context::r_error("L-BFGS-B workspace exceeds supported dimensions")
        });
        load_gcall(&mut os, gr);
        if Rf_isNull(gr) != 0 {
            load_ndeps(&mut os, &scratch, options, npar);
        }
        let _gcall_guard = protect(os.R_gcall);
        args = CDR(args);
        let slower = CAR(args);
        args = CDR(args);
        let supper = CAR(args);

        let lower: &mut [f64] = vect(&scratch, n_usize);
        let upper: &mut [f64] = vect(&scratch, n_usize);
        let nbd: &mut [c_int] = vect(&scratch, n_usize);
        for i in 0..n_usize {
            lower[i] = REAL_ELT(slower, i as c_int) / os.parscale[i];
            upper[i] = REAL_ELT(supper, i as c_int) / os.parscale[i];
            nbd[i] = match (R_FINITE(lower[i]), R_FINITE(upper[i])) {
                (false, false) => 0,
                (false, true) => 3,
                (true, false) => 1,
                (true, true) => 2,
            };
        }
        os.usebounds = 1;
        os.lower = lower;
        os.upper = upper;

        let wa: &mut [f64] = vect(&scratch, wa_len.max(0) as usize);
        let iwa: &mut [c_int] = vect(&scratch, iwa_len.max(0) as usize);
        let g: &mut [f64] = vect(&scratch, n_usize);
        let mut task: [c_char; 60] = [0; 60];
        let mut pgtol_val = pgtol;
        let mut isave: [c_int; 44] = [0; 44];
        let mut lmsg: [u8; 60] = [0; 60];

        // Initialize task to "START"
        for (i, &b) in b"START\0".iter().enumerate() {
            task[i] = b as c_char;
        }

        // Map trace to lbfgsb's iprint exactly as appl/optim.c's lbfgsb
        // wrapper does: silent (-1) unless trace >= 2.
        let iprint = match trace {
            2 => 0,
            3 => nREPORT,
            4 => 99,
            5 => 100,
            6 => 101,
            _ => -1,
        };
        // Stateful loop mirroring the C wrapper in appl/optim.c: evaluate
        // f/g only on FG* tasks, count iterations on NEW_X, and stop only
        // when iter exceeds maxit.
        let mut l_iter: c_int = 0;
        let mut state = LbfgsbState::default();
        loop {
            // SAFETY: every buffer is live scratch sized for `npar`/`lmm`
            // (`workspace_lengths` sizes `wa`/`iwa`), `task` is a 60-byte
            // NUL-terminated buffer and `isave` has the 44 slots lbfgsb uses.
            // lbfgsb only reads the bounds `l`/`u`, so sharing them with
            // `os` (read by the callbacks between calls) is sound.
            unsafe {
                lbfgsb(
                    &mut state,
                    npar,
                    lmm,
                    dpar.as_mut_ptr(),
                    os.lower.as_ptr().cast_mut(),
                    os.upper.as_ptr().cast_mut(),
                    nbd.as_mut_ptr(),
                    &mut val,
                    g.as_mut_ptr(),
                    factr,
                    &mut pgtol_val,
                    wa.as_mut_ptr(),
                    iwa.as_mut_ptr(),
                    task.as_mut_ptr(),
                    iprint,
                    isave.as_mut_ptr(),
                );
            }

            // Read task as bytes
            let task_len = task.iter().position(|&c| c == 0).unwrap_or(60);
            let task_bytes: Vec<u8> = task[..task_len].iter().map(|&c| c as u8).collect();
            let copy_msg = |lmsg: &mut [u8; 60]| {
                lmsg[..task_len].copy_from_slice(&task_bytes);
            };

            if task_bytes.starts_with(b"FG") {
                // FG_START or FG or FG_LNSRCH: compute f and g. GNU's
                // lbfgsb() wrapper errors on a non-finite fn value.
                val = objective(&os, dpar);
                if !R_FINITE(val) {
                    Rf_error_cstr(c"L-BFGS-B needs finite values of 'fn'");
                }
                gradient(&os, dpar, g);
            } else if task_bytes.starts_with(b"NEW_X") {
                l_iter += 1;
                if trace == 1 && (l_iter % nREPORT == 0) {
                    rprintf(&format!("iter {:4} value {:.6}\n", l_iter, val));
                }
                if l_iter > maxit {
                    ifail = 1; // iteration limit reached (GNU: *fail = 1)
                    copy_msg(&mut lmsg);
                    fncount = isave[33]; // nfgv (function/gradient count)
                    grcount = fncount;
                    break;
                }
            } else {
                ifail = if task_bytes.starts_with(b"WARN") {
                    51
                } else if task_bytes.starts_with(b"CONV") {
                    ifail
                } else {
                    // ERROR, or some other condition that is not supposed
                    // to happen
                    52
                };
                copy_msg(&mut lmsg);
                fncount = isave[33];
                grcount = fncount;
                break;
            }
        }

        if trace != 0 {
            rprintf(&format!("final  value {:.6} \n", val));
            let line2 = if l_iter < maxit && ifail == 0 {
                "converged\n".to_string()
            } else {
                format!("stopped after {} iterations\n", l_iter)
            };
            rprintf(&line2);
        }

        store_par(par, dpar, &os);
        let msg_len = lmsg.iter().position(|&c| c == 0).unwrap_or(60);
        let msg_str = String::from_utf8_lossy(&lmsg[..msg_len]);
        let smsg = mk_string(msg_str.as_ref());
        let _smsg_guard = protect(smsg);
        SET_VECTOR_ELT(res, 4, smsg);
    } else {
        Rf_error_cstr(c"unknown 'method'");
    }

    if Rf_isNull(os.names) == 0 {
        setAttrib(par, R_NamesSymbol(), os.names);
    }
    SET_REAL_ELT(value, 0, val * os.fnscale);
    SET_VECTOR_ELT(res, 0, par);
    SET_VECTOR_ELT(res, 1, value);
    SET_INTEGER_ELT(counts, 0, fncount);
    SET_INTEGER_ELT(counts, 1, grcount);
    SET_VECTOR_ELT(res, 2, counts);
    SET_INTEGER_ELT(conv, 0, ifail);
    SET_VECTOR_ELT(res, 3, conv);
    res
}

// ---------------------------------------------------------------------------
// optimhess -- numerical Hessian computation
// ---------------------------------------------------------------------------

pub fn optimhess(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let scratch = optimization_scope();
    let mut args = CDR(args);
    let mut os = OptStruct::new(rho);

    let par = CAR(args);
    let npar = LENGTH(par);
    let n_usize = npar.max(0) as usize;
    os.names = getAttrib(par, R_NamesSymbol());
    args = CDR(args);
    let fn_sexp = CAR(args);
    if !is_function(fn_sexp) {
        Rf_error_cstr(c"'fn' is not a function");
    }
    args = CDR(args);
    let gr = CAR(args);
    args = CDR(args);
    let options = CAR(args);

    os.fnscale = as_real(getListElement(options, "fnscale"));
    let tmp = getListElement(options, "parscale");
    if LENGTH(tmp) != npar {
        Rf_error_cstr(c"'parscale' is of the wrong length");
    }
    os.parscale = real_option(&scratch, tmp, npar);

    os.R_fcall = Rf_lang2(fn_sexp, R_NilValue());
    let _fcall_guard = protect(os.R_fcall);

    let par = coerceVector(par, SEXPTYPE::REALSXP.as_c_int());
    let _par_guard = protect(par);

    load_gcall(&mut os, gr);
    let _gcall_guard = protect(os.R_gcall);

    load_ndeps(&mut os, &scratch, options, npar);

    let ans = allocMatrix(SEXPTYPE::REALSXP.into(), npar, npar);
    let _ans_guard = protect(ans);
    let dpar: &mut [f64] = vect(&scratch, n_usize);
    for i in 0..n_usize {
        dpar[i] = REAL_ELT(par, i as c_int) / os.parscale[i];
    }
    let df1: &mut [f64] = vect(&scratch, n_usize);
    let df2: &mut [f64] = vect(&scratch, n_usize);
    for i in 0..n_usize {
        let eps = os.ndeps[i] / os.parscale[i];
        dpar[i] += eps;
        gradient(&os, dpar, df1);
        dpar[i] -= 2.0 * eps;
        gradient(&os, dpar, df2);
        for j in 0..n_usize {
            SET_REAL_ELT(
                ans,
                (i * n_usize + j) as c_int,
                os.fnscale * (df1[j] - df2[j]) / (2.0 * eps * os.parscale[i] * os.parscale[j]),
            );
        }
        dpar[i] += eps;
    }

    // Symmetrize
    for i in 0..n_usize {
        for j in 0..i {
            let (ij, ji) = ((i * n_usize + j) as c_int, (j * n_usize + i) as c_int);
            let tmp_val = 0.5 * (REAL_ELT(ans, ij) + REAL_ELT(ans, ji));
            SET_REAL_ELT(ans, ij, tmp_val);
            SET_REAL_ELT(ans, ji, tmp_val);
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

pub unsafe extern "C-unwind" fn c_optim(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    optim(call, op, args, rho)
}

pub unsafe extern "C-unwind" fn c_optimhess(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    optimhess(call, op, args, rho)
}

pub fn do_optim(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let par = CAR(args);
    let fn_sexp = CAR(CDR(args));
    let method = mk_string("Nelder-Mead");
    let _m = protect(method);
    let options = Rf_allocVector3(SEXPTYPE::VECSXP, 9);
    let _o = protect(options);
    SET_VECTOR_ELT(options, 0, Rf_ScalarInteger(500));
    let npar = XLENGTH(par).max(1);
    let parscale = Rf_allocVector3(SEXPTYPE::REALSXP, npar);
    let _ps = protect(parscale);
    super::checked::set_reals(parscale, &vec![1.0; npar as usize]);
    SET_VECTOR_ELT(options, 1, parscale);
    SET_VECTOR_ELT(options, 2, Rf_ScalarReal(1.0));
    SET_VECTOR_ELT(options, 3, Rf_ScalarReal(f64::NEG_INFINITY));
    SET_VECTOR_ELT(options, 4, Rf_ScalarReal(f64::EPSILON.sqrt()));
    SET_VECTOR_ELT(options, 5, Rf_ScalarReal(1.0));
    SET_VECTOR_ELT(options, 6, Rf_ScalarReal(0.5));
    SET_VECTOR_ELT(options, 7, Rf_ScalarReal(2.0));
    SET_VECTOR_ELT(options, 8, Rf_ScalarInteger(0));
    crate::mainutils::essentials::set_string_names(
        options,
        &[
            "maxit".to_string(),
            "parscale".to_string(),
            "fnscale".to_string(),
            "abstol".to_string(),
            "reltol".to_string(),
            "alpha".to_string(),
            "beta".to_string(),
            "gamma".to_string(),
            "trace".to_string(),
        ],
    );
    let internal = Rf_cons(
        R_NilValue(),
        Rf_cons(
            par,
            Rf_cons(
                fn_sexp,
                Rf_cons(
                    R_NilValue(),
                    Rf_cons(method, Rf_cons(options, R_NilValue())),
                ),
            ),
        ),
    );
    let _i = protect(internal);
    optim(call, op, internal, rho)
}

/// GNU `constrOptim(theta, f, grad, ui, ci)` — log-barrier + Nelder-Mead.
pub fn do_constr_optim(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let par = CAR(args);
    let fn_sexp = CAR(CDR(args));
    let mut cell = CDR(CDR(args));
    if !cell.is_null() && cell != R_NilValue() {
        cell = CDR(cell);
    }
    let ui = if cell.is_null() || cell == R_NilValue() {
        R_NilValue()
    } else {
        CAR(cell)
    };
    if !cell.is_null() && cell != R_NilValue() {
        cell = CDR(cell);
    }
    let ci = if cell.is_null() || cell == R_NilValue() {
        R_NilValue()
    } else {
        CAR(cell)
    };
    let npar = XLENGTH(par) as usize;
    if npar == 0 || fn_sexp.is_null() || fn_sexp == R_NilValue() {
        return R_NilValue();
    }
    let dim = crate::sexp::attrib_core::getAttrib(ui, crate::sexp::attrib_core::R_DimSymbol());
    let (ncons, ui_npar) = if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) >= 2 {
        (INTEGER_ELT(dim, 0) as usize, INTEGER_ELT(dim, 1) as usize)
    } else if ui.is_null() || ui == R_NilValue() {
        (0, 0)
    } else {
        (XLENGTH(ui) as usize, npar)
    };
    if ncons == 0 || ui_npar != npar || XLENGTH(ci) as usize != ncons {
        crate::mainutils::errors::errorcall_str(
            call,
            "initial value is not in the interior of the feasible region",
        );
    }
    let mut theta: Vec<f64> = (0..npar).map(|i| REAL_ELT(par, i as c_int)).collect();
    let mut ui_m = vec![0.0; ncons * npar];
    let mut ci_v = vec![0.0; ncons];
    for j in 0..npar {
        for i in 0..ncons {
            ui_m[i + j * ncons] =
                crate::mainutils::essentials::elt_real_safe(ui, (i + j * ncons) as i64);
        }
    }
    for i in 0..ncons {
        ci_v[i] = crate::mainutils::essentials::elt_real_safe(ci, i as i64);
    }
    fn slack(ui_m: &[f64], ci_v: &[f64], th: &[f64], ncons: usize, npar: usize) -> Vec<f64> {
        let mut g = vec![0.0; ncons];
        for i in 0..ncons {
            let mut s = 0.0;
            for j in 0..npar {
                s += ui_m[i + j * ncons] * th[j];
            }
            g[i] = s - ci_v[i];
        }
        g
    }
    let g0 = slack(&ui_m, &ci_v, &theta, ncons, npar);
    if g0.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        crate::mainutils::errors::errorcall_str(
            call,
            "initial value is not in the interior of the feasible region",
        );
    }
    fn eval_f(fn_sexp: SEXP, th: &[f64], rho: SEXP) -> f64 {
        let tv = Rf_allocVector3(SEXPTYPE::REALSXP, th.len() as i64);
        let _t = protect(tv);
        super::checked::set_reals(tv, th);
        let call = Rf_lang2(fn_sexp, tv);
        let _c = protect(call);
        let val = eval(call, rho);
        crate::mainutils::essentials::elt_real_safe(val, 0)
    }
    fn barrier(
        fn_sexp: SEXP,
        th: &[f64],
        told: &[f64],
        ui_m: &[f64],
        ci_v: &[f64],
        ncons: usize,
        npar: usize,
        mu: f64,
        rho: SEXP,
    ) -> f64 {
        let gi = slack(ui_m, ci_v, th, ncons, npar);
        if gi.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return f64::NAN;
        }
        let gi_old = slack(ui_m, ci_v, told, ncons, npar);
        let mut bar = 0.0;
        for i in 0..ncons {
            let mut uith = 0.0;
            for j in 0..npar {
                uith += ui_m[i + j * ncons] * th[j];
            }
            bar += gi_old[i] * gi[i].ln() - uith;
        }
        let fval = eval_f(fn_sexp, th, rho);
        if !fval.is_finite() || !bar.is_finite() {
            return f64::NAN;
        }
        fval - mu * bar
    }
    fn nelder(
        fn_sexp: SEXP,
        start: &[f64],
        told: &[f64],
        ui_m: &[f64],
        ci_v: &[f64],
        ncons: usize,
        npar: usize,
        mu: f64,
        rho: SEXP,
    ) -> (Vec<f64>, f64) {
        let n = npar;
        let mut pts = vec![start.to_vec(); n + 1];
        for i in 0..n {
            let step = if start[i].abs() > 1e-8 {
                0.05 * start[i].abs()
            } else {
                0.05
            };
            pts[i + 1][i] += step;
        }
        let mut vals: Vec<f64> = pts
            .iter()
            .map(|p| barrier(fn_sexp, p, told, ui_m, ci_v, ncons, npar, mu, rho))
            .collect();
        for _ in 0..400 {
            let mut order: Vec<usize> = (0..=n).collect();
            order.sort_by(|&a, &b| {
                vals[a]
                    .partial_cmp(&vals[b])
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let best = order[0];
            let worst = order[n];
            let sworst = order[n - 1];
            let mut cen = vec![0.0; n];
            for &k in &order[..n] {
                for j in 0..n {
                    cen[j] += pts[k][j];
                }
            }
            for j in 0..n {
                cen[j] /= n as f64;
            }
            let mut refl = vec![0.0; n];
            for j in 0..n {
                refl[j] = 2.0 * cen[j] - pts[worst][j];
            }
            let fr = barrier(fn_sexp, &refl, told, ui_m, ci_v, ncons, npar, mu, rho);
            if fr.is_finite() && fr < vals[best] {
                let mut expn = vec![0.0; n];
                for j in 0..n {
                    expn[j] = cen[j] + 2.0 * (refl[j] - cen[j]);
                }
                let fe = barrier(fn_sexp, &expn, told, ui_m, ci_v, ncons, npar, mu, rho);
                if fe.is_finite() && fe < fr {
                    pts[worst] = expn;
                    vals[worst] = fe;
                } else {
                    pts[worst] = refl;
                    vals[worst] = fr;
                }
            } else if fr.is_finite() && fr < vals[sworst] {
                pts[worst] = refl;
                vals[worst] = fr;
            } else {
                let dir = if fr.is_finite() && fr < vals[worst] {
                    &refl
                } else {
                    &pts[worst]
                };
                let mut contr = vec![0.0; n];
                for j in 0..n {
                    contr[j] = cen[j] + 0.5 * (dir[j] - cen[j]);
                }
                let fc = barrier(fn_sexp, &contr, told, ui_m, ci_v, ncons, npar, mu, rho);
                if fc.is_finite() && fc < vals[worst] {
                    pts[worst] = contr;
                    vals[worst] = fc;
                } else {
                    for i in 1..=n {
                        for j in 0..n {
                            pts[i][j] = pts[best][j] + 0.5 * (pts[i][j] - pts[best][j]);
                        }
                        vals[i] = barrier(fn_sexp, &pts[i], told, ui_m, ci_v, ncons, npar, mu, rho);
                    }
                }
            }
            let mut span: f64 = 0.0;
            for i in 0..=n {
                for j in 0..n {
                    span = span.max((pts[i][j] - pts[best][j]).abs());
                }
            }
            if span < 1e-8 {
                break;
            }
        }
        let mut best_i = 0;
        for i in 1..=n {
            if vals[i].is_finite() && (!vals[best_i].is_finite() || vals[i] < vals[best_i]) {
                best_i = i;
            }
        }
        (pts[best_i].clone(), vals[best_i])
    }
    let mu = 1e-4;
    let mut told = theta.clone();
    let mut r_old = barrier(fn_sexp, &theta, &told, &ui_m, &ci_v, ncons, npar, mu, rho);
    for _ in 0..40 {
        let (nth, r) = nelder(fn_sexp, &theta, &told, &ui_m, &ci_v, ncons, npar, mu, rho);
        if r.is_finite() && r_old.is_finite() && (r - r_old).abs() < (0.001 + r.abs()) * 1e-5 {
            theta = nth;
            break;
        }
        if r.is_finite() {
            theta = nth;
            r_old = r;
        }
        told.clone_from(&theta);
    }
    let fval = eval_f(fn_sexp, &theta, rho);
    let par_out = Rf_allocVector3(SEXPTYPE::REALSXP, npar as i64);
    let _p = protect(par_out);
    super::checked::set_reals(par_out, &theta);
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    let _r = protect(result);
    SET_VECTOR_ELT(result, 0, par_out);
    SET_VECTOR_ELT(result, 1, Rf_ScalarReal(fval));
    crate::mainutils::essentials::set_string_names(
        result,
        &["par".to_string(), "value".to_string()],
    );
    result
}

/// GNU `optimHess(par, fn)` — numerical Hessian with `ndeps=0.001`.
pub fn do_optim_hess(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let par = CAR(args);
    let fn_sexp = CAR(CDR(args));
    let npar = XLENGTH(par) as i32;
    if npar < 1 {
        return R_NilValue();
    }
    let parscale = Rf_allocVector3(SEXPTYPE::REALSXP, npar as i64);
    let _ps = protect(parscale);
    let ndeps = Rf_allocVector3(SEXPTYPE::REALSXP, npar as i64);
    let _nd = protect(ndeps);
    super::checked::set_reals(parscale, &vec![1.0; npar as usize]);
    super::checked::set_reals(ndeps, &vec![0.001; npar as usize]);
    let options = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
    let _o = protect(options);
    SET_VECTOR_ELT(options, 0, Rf_ScalarReal(1.0));
    SET_VECTOR_ELT(options, 1, parscale);
    SET_VECTOR_ELT(options, 2, ndeps);
    crate::mainutils::essentials::set_string_names(
        options,
        &[
            "fnscale".to_string(),
            "parscale".to_string(),
            "ndeps".to_string(),
        ],
    );
    let internal = Rf_cons(
        R_NilValue(),
        Rf_cons(
            par,
            Rf_cons(
                fn_sexp,
                Rf_cons(R_NilValue(), Rf_cons(options, R_NilValue())),
            ),
        ),
    );
    let _i = protect(internal);
    optimhess(call, op, internal, rho)
}
