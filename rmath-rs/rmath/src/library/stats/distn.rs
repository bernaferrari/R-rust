#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1998--2025  The R Core Team
 *
 *  distn ==  [DIST]ributio[N]s, i.e. probability distributions
 *
 *  Ported to Rust from r-source/src/library/stats/src/distn.c
 */

use std::os::raw::{c_double, c_int};
use std::ptr;

use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// External declarations
// ---------------------------------------------------------------------------

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::main::coerce::coerceVector(x, type_)
}

// ---------------------------------------------------------------------------
// Helper: isNumeric
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
unsafe fn isNumeric(x: SEXP) -> bool {
    if x.is_null() {
        return false;
    }
    let t = TYPEOF(x);
    t == SEXPTYPE::REALSXP.0 || t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0
}

// ---------------------------------------------------------------------------
// Helper: asInteger
// ---------------------------------------------------------------------------

unsafe fn as_integer(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::INTSXP.0 {
        return *INTEGER(x);
    }
    if t == SEXPTYPE::REALSXP.0 {
        let v = *REAL(x);
        if v.is_nan() || v < c_int::MIN as c_double || v > c_int::MAX as c_double {
            return NA_INTEGER;
        }
        return v as c_int;
    }
    if t == SEXPTYPE::LGLSXP.0 {
        return *INTEGER(x);
    }
    NA_INTEGER
}

// ---------------------------------------------------------------------------
// Math2 helpers (2-argument math functions)
// ---------------------------------------------------------------------------

type math2_fn_1 = unsafe fn(c_double, c_double, c_int) -> c_double;
type math2_fn_2 = unsafe fn(c_double, c_double, c_int, c_int) -> c_double;

unsafe fn math2_1(sa: SEXP, sb: SEXP, sI: SEXP, f: math2_fn_1) -> SEXP {
    if !isNumeric(sa) || !isNumeric(sb) {
        Rf_error(b"Non-numeric argument to mathematical function\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let na = XLENGTH(sa);
    let nb = XLENGTH(sb);
    if na == 0 || nb == 0 {
        let sy = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, 0));
        Rf_unprotect(1);
        return sy;
    }

    let n = if na < nb { nb } else { na };
    let sa = Rf_protect(coerceVector(sa, SEXPTYPE::REALSXP.0));
    let sb = Rf_protect(coerceVector(sb, SEXPTYPE::REALSXP.0));
    let sy = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int));
    let a = REAL(sa);
    let b = REAL(sb);
    let y = REAL(sy);
    let mut naflag = false;

    let m_opt = as_integer(sI);
    let mut ia: R_xlen_t = 0;
    let mut ib: R_xlen_t = 0;

    for i in 0..n {
        let ai = *a.add(ia as usize);
        let bi = *b.add(ib as usize);

        if R_IsNA(ai) || R_IsNA(bi) {
            *y.add(i as usize) = NA_REAL;
        } else if ISNAN(ai) || ISNAN(bi) {
            *y.add(i as usize) = 0.0 / 0.0; // R_NaN
        } else {
            *y.add(i as usize) = f(ai, bi, m_opt);
            if ISNAN(*y.add(i as usize)) {
                naflag = true;
            }
        }

        ia += 1;
        if ia >= na {
            ia = 0;
        }
        ib += 1;
        if ib >= nb {
            ib = 0;
        }
    }

    if naflag {
        eprintln!("NaNs produced");
    }
    Rf_unprotect(3);
    sy
}

unsafe fn math2_2(sa: SEXP, sb: SEXP, sI1: SEXP, sI2: SEXP, f: math2_fn_2) -> SEXP {
    if !isNumeric(sa) || !isNumeric(sb) {
        Rf_error(b"Non-numeric argument to mathematical function\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let na = XLENGTH(sa);
    let nb = XLENGTH(sb);
    if na == 0 || nb == 0 {
        let sy = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, 0));
        Rf_unprotect(1);
        return sy;
    }

    let n = if na < nb { nb } else { na };
    let sa = Rf_protect(coerceVector(sa, SEXPTYPE::REALSXP.0));
    let sb = Rf_protect(coerceVector(sb, SEXPTYPE::REALSXP.0));
    let sy = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int));
    let a = REAL(sa);
    let b = REAL(sb);
    let y = REAL(sy);
    let mut naflag = false;

    let i_1 = as_integer(sI1);
    let i_2 = as_integer(sI2);
    let mut ia: R_xlen_t = 0;
    let mut ib: R_xlen_t = 0;

    for i in 0..n {
        let ai = *a.add(ia as usize);
        let bi = *b.add(ib as usize);

        if R_IsNA(ai) || R_IsNA(bi) {
            *y.add(i as usize) = NA_REAL;
        } else if ISNAN(ai) || ISNAN(bi) {
            *y.add(i as usize) = 0.0 / 0.0;
        } else {
            *y.add(i as usize) = f(ai, bi, i_1, i_2);
            if ISNAN(*y.add(i as usize)) {
                naflag = true;
            }
        }

        ia += 1;
        if ia >= na {
            ia = 0;
        }
        ib += 1;
        if ib >= nb {
            ib = 0;
        }
    }

    if naflag {
        eprintln!("NaNs produced");
    }
    Rf_unprotect(3);
    sy
}

// ---------------------------------------------------------------------------
// Math3 helpers (3-argument math functions)
// ---------------------------------------------------------------------------

type math3_fn_1 = unsafe fn(c_double, c_double, c_double, c_int) -> c_double;
type math3_fn_2 = unsafe fn(c_double, c_double, c_double, c_int, c_int) -> c_double;

unsafe fn math3_1(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, f: math3_fn_1) -> SEXP {
    if !isNumeric(sa) || !isNumeric(sb) || !isNumeric(sc) {
        Rf_error(b"Non-numeric argument to mathematical function\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let na = XLENGTH(sa);
    let nb = XLENGTH(sb);
    let nc = XLENGTH(sc);
    if na == 0 || nb == 0 || nc == 0 {
        let sy = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, 0));
        Rf_unprotect(1);
        return sy;
    }

    let mut n = na;
    if n < nb {
        n = nb;
    }
    if n < nc {
        n = nc;
    }

    let sa = Rf_protect(coerceVector(sa, SEXPTYPE::REALSXP.0));
    let sb = Rf_protect(coerceVector(sb, SEXPTYPE::REALSXP.0));
    let sc = Rf_protect(coerceVector(sc, SEXPTYPE::REALSXP.0));
    let sy = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int));
    let a = REAL(sa);
    let b = REAL(sb);
    let c = REAL(sc);
    let y = REAL(sy);
    let mut naflag = false;

    let i_1 = as_integer(sI);
    let mut ia: R_xlen_t = 0;
    let mut ib: R_xlen_t = 0;
    let mut ic: R_xlen_t = 0;

    for i in 0..n {
        let ai = *a.add(ia as usize);
        let bi = *b.add(ib as usize);
        let ci = *c.add(ic as usize);

        if R_IsNA(ai) || R_IsNA(bi) || R_IsNA(ci) {
            *y.add(i as usize) = NA_REAL;
        } else if ISNAN(ai) || ISNAN(bi) || ISNAN(ci) {
            *y.add(i as usize) = 0.0 / 0.0;
        } else {
            *y.add(i as usize) = f(ai, bi, ci, i_1);
            if ISNAN(*y.add(i as usize)) {
                naflag = true;
            }
        }

        ia += 1;
        if ia >= na {
            ia = 0;
        }
        ib += 1;
        if ib >= nb {
            ib = 0;
        }
        ic += 1;
        if ic >= nc {
            ic = 0;
        }
    }

    if naflag {
        eprintln!("NaNs produced");
    }
    Rf_unprotect(4);
    sy
}

unsafe fn math3_2(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP, f: math3_fn_2) -> SEXP {
    if !isNumeric(sa) || !isNumeric(sb) || !isNumeric(sc) {
        Rf_error(b"Non-numeric argument to mathematical function\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let na = XLENGTH(sa);
    let nb = XLENGTH(sb);
    let nc = XLENGTH(sc);
    if na == 0 || nb == 0 || nc == 0 {
        let sy = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, 0));
        Rf_unprotect(1);
        return sy;
    }

    let mut n = na;
    if n < nb {
        n = nb;
    }
    if n < nc {
        n = nc;
    }

    let sa = Rf_protect(coerceVector(sa, SEXPTYPE::REALSXP.0));
    let sb = Rf_protect(coerceVector(sb, SEXPTYPE::REALSXP.0));
    let sc = Rf_protect(coerceVector(sc, SEXPTYPE::REALSXP.0));
    let sy = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int));
    let a = REAL(sa);
    let b = REAL(sb);
    let c = REAL(sc);
    let y = REAL(sy);
    let mut naflag = false;

    let i_1 = as_integer(sI);
    let i_2 = as_integer(sJ);
    let mut ia: R_xlen_t = 0;
    let mut ib: R_xlen_t = 0;
    let mut ic: R_xlen_t = 0;

    for i in 0..n {
        let ai = *a.add(ia as usize);
        let bi = *b.add(ib as usize);
        let ci = *c.add(ic as usize);

        if R_IsNA(ai) || R_IsNA(bi) || R_IsNA(ci) {
            *y.add(i as usize) = NA_REAL;
        } else if ISNAN(ai) || ISNAN(bi) || ISNAN(ci) {
            *y.add(i as usize) = 0.0 / 0.0;
        } else {
            *y.add(i as usize) = f(ai, bi, ci, i_1, i_2);
            if ISNAN(*y.add(i as usize)) {
                naflag = true;
            }
        }

        ia += 1;
        if ia >= na {
            ia = 0;
        }
        ib += 1;
        if ib >= nb {
            ib = 0;
        }
        ic += 1;
        if ic >= nc {
            ic = 0;
        }
    }

    if naflag {
        eprintln!("NaNs produced");
    }
    Rf_unprotect(4);
    sy
}

// ---------------------------------------------------------------------------
// Math4 helpers (4-argument math functions)
// ---------------------------------------------------------------------------

type math4_fn_1 = unsafe fn(c_double, c_double, c_double, c_double, c_int) -> c_double;
type math4_fn_2 = unsafe fn(c_double, c_double, c_double, c_double, c_int, c_int) -> c_double;

unsafe fn math4_1(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP, f: math4_fn_1) -> SEXP {
    if !isNumeric(sa) || !isNumeric(sb) || !isNumeric(sc) || !isNumeric(sd) {
        Rf_error(b"Non-numeric argument to mathematical function\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let na = XLENGTH(sa);
    let nb = XLENGTH(sb);
    let nc = XLENGTH(sc);
    let nd = XLENGTH(sd);
    if na == 0 || nb == 0 || nc == 0 || nd == 0 {
        let sy = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, 0));
        Rf_unprotect(1);
        return sy;
    }

    let mut n = na;
    if n < nb {
        n = nb;
    }
    if n < nc {
        n = nc;
    }
    if n < nd {
        n = nd;
    }

    let sa = Rf_protect(coerceVector(sa, SEXPTYPE::REALSXP.0));
    let sb = Rf_protect(coerceVector(sb, SEXPTYPE::REALSXP.0));
    let sc = Rf_protect(coerceVector(sc, SEXPTYPE::REALSXP.0));
    let sd = Rf_protect(coerceVector(sd, SEXPTYPE::REALSXP.0));
    let sy = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int));
    let a = REAL(sa);
    let b = REAL(sb);
    let c = REAL(sc);
    let d = REAL(sd);
    let y = REAL(sy);
    let mut naflag = false;

    let i_1 = as_integer(sI);
    let mut ia: R_xlen_t = 0;
    let mut ib: R_xlen_t = 0;
    let mut ic: R_xlen_t = 0;
    let mut id: R_xlen_t = 0;

    for i in 0..n {
        let ai = *a.add(ia as usize);
        let bi = *b.add(ib as usize);
        let ci = *c.add(ic as usize);
        let di = *d.add(id as usize);

        if R_IsNA(ai) || R_IsNA(bi) || R_IsNA(ci) || R_IsNA(di) {
            *y.add(i as usize) = NA_REAL;
        } else if ISNAN(ai) || ISNAN(bi) || ISNAN(ci) || ISNAN(di) {
            *y.add(i as usize) = 0.0 / 0.0;
        } else {
            *y.add(i as usize) = f(ai, bi, ci, di, i_1);
            if ISNAN(*y.add(i as usize)) {
                naflag = true;
            }
        }

        ia += 1;
        if ia >= na {
            ia = 0;
        }
        ib += 1;
        if ib >= nb {
            ib = 0;
        }
        ic += 1;
        if ic >= nc {
            ic = 0;
        }
        id += 1;
        if id >= nd {
            id = 0;
        }
    }

    if naflag {
        eprintln!("NaNs produced");
    }
    Rf_unprotect(5);
    sy
}

unsafe fn math4_2(
    sa: SEXP,
    sb: SEXP,
    sc: SEXP,
    sd: SEXP,
    sI: SEXP,
    sJ: SEXP,
    f: math4_fn_2,
) -> SEXP {
    if !isNumeric(sa) || !isNumeric(sb) || !isNumeric(sc) || !isNumeric(sd) {
        Rf_error(b"Non-numeric argument to mathematical function\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let na = XLENGTH(sa);
    let nb = XLENGTH(sb);
    let nc = XLENGTH(sc);
    let nd = XLENGTH(sd);
    if na == 0 || nb == 0 || nc == 0 || nd == 0 {
        let sy = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, 0));
        Rf_unprotect(1);
        return sy;
    }

    let mut n = na;
    if n < nb {
        n = nb;
    }
    if n < nc {
        n = nc;
    }
    if n < nd {
        n = nd;
    }

    let sa = Rf_protect(coerceVector(sa, SEXPTYPE::REALSXP.0));
    let sb = Rf_protect(coerceVector(sb, SEXPTYPE::REALSXP.0));
    let sc = Rf_protect(coerceVector(sc, SEXPTYPE::REALSXP.0));
    let sd = Rf_protect(coerceVector(sd, SEXPTYPE::REALSXP.0));
    let sy = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int));
    let a = REAL(sa);
    let b = REAL(sb);
    let c = REAL(sc);
    let d = REAL(sd);
    let y = REAL(sy);
    let mut naflag = false;

    let i_1 = as_integer(sI);
    let i_2 = as_integer(sJ);
    let mut ia: R_xlen_t = 0;
    let mut ib: R_xlen_t = 0;
    let mut ic: R_xlen_t = 0;
    let mut id: R_xlen_t = 0;

    for i in 0..n {
        let ai = *a.add(ia as usize);
        let bi = *b.add(ib as usize);
        let ci = *c.add(ic as usize);
        let di = *d.add(id as usize);

        if R_IsNA(ai) || R_IsNA(bi) || R_IsNA(ci) || R_IsNA(di) {
            *y.add(i as usize) = NA_REAL;
        } else if ISNAN(ai) || ISNAN(bi) || ISNAN(ci) || ISNAN(di) {
            *y.add(i as usize) = 0.0 / 0.0;
        } else {
            *y.add(i as usize) = f(ai, bi, ci, di, i_1, i_2);
            if ISNAN(*y.add(i as usize)) {
                naflag = true;
            }
        }

        ia += 1;
        if ia >= na {
            ia = 0;
        }
        ib += 1;
        if ib >= nb {
            ib = 0;
        }
        ic += 1;
        if ic >= nc {
            ic = 0;
        }
        id += 1;
        if id >= nd {
            id = 0;
        }
    }

    if naflag {
        eprintln!("NaNs produced");
    }
    Rf_unprotect(5);
    sy
}

// ===========================================================================
// Wrapper functions that adapt R's d/p/q function signatures
// The C functions use (int give_log, int lower_tail, int log_p) but our
// inner functions use bool. These wrappers adapt between the two.
// ===========================================================================

// --- dchisq(x, df, log) ---

unsafe fn wrap_dchisq(x: c_double, df: c_double, log: c_int) -> c_double {
    crate::nmath::dist::chisq::dchisq_inner(x, df, log != 0)
}

pub unsafe fn do_dchisq(sa: SEXP, sb: SEXP, sI: SEXP) -> SEXP {
    math2_1(sa, sb, sI, wrap_dchisq)
}

// --- dexp(x, scale, log) ---

unsafe fn wrap_dexp(x: c_double, scale: c_double, log: c_int) -> c_double {
    crate::nmath::dist::exponential::dexp_inner(x, scale, log != 0)
}

pub unsafe fn do_dexp(sa: SEXP, sb: SEXP, sI: SEXP) -> SEXP {
    math2_1(sa, sb, sI, wrap_dexp)
}

// --- dgeom(x, p, log) ---

unsafe fn wrap_dgeom(x: c_double, p: c_double, log: c_int) -> c_double {
    crate::nmath::dist::geometric::dgeom_inner(x, p, log != 0)
}

pub unsafe fn do_dgeom(sa: SEXP, sb: SEXP, sI: SEXP) -> SEXP {
    math2_1(sa, sb, sI, wrap_dgeom)
}

// --- dpois(x, lambda, log) ---

unsafe fn wrap_dpois(x: c_double, lambda: c_double, log: c_int) -> c_double {
    crate::nmath::dist::poisson::dpois_inner(x, lambda, log != 0)
}

pub unsafe fn do_dpois(sa: SEXP, sb: SEXP, sI: SEXP) -> SEXP {
    math2_1(sa, sb, sI, wrap_dpois)
}

// --- dt(x, df, log) ---

unsafe fn wrap_dt(x: c_double, df: c_double, log: c_int) -> c_double {
    crate::nmath::dist::t_dist::dt_inner(x, df, log != 0)
}

pub unsafe fn do_dt(sa: SEXP, sb: SEXP, sI: SEXP) -> SEXP {
    math2_1(sa, sb, sI, wrap_dt)
}

// --- dsignrank(x, n, log) ---

unsafe fn wrap_dsignrank(x: c_double, n: c_double, log: c_int) -> c_double {
    crate::nmath::dist::signrank::dsignrank_inner(x, n, log != 0)
}

pub unsafe fn do_dsignrank(sa: SEXP, sb: SEXP, sI: SEXP) -> SEXP {
    math2_1(sa, sb, sI, wrap_dsignrank)
}

// --- pchisq(x, df, lower_tail, log_p) ---

unsafe fn wrap_pchisq(x: c_double, df: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::chisq::pchisq_inner(x, df, lt != 0, lp != 0)
}

pub unsafe fn do_pchisq(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, wrap_pchisq)
}

// --- qchisq(p, df, lower_tail, log_p) ---

unsafe fn wrap_qchisq(p: c_double, df: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::chisq::qchisq_inner(p, df, lt != 0, lp != 0)
}

pub unsafe fn do_qchisq(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, wrap_qchisq)
}

// --- pexp(x, scale, lower_tail, log_p) ---

unsafe fn wrap_pexp(x: c_double, scale: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::exponential::pexp_inner(x, scale, lt != 0, lp != 0)
}

pub unsafe fn do_pexp(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, wrap_pexp)
}

// --- qexp(p, scale, lower_tail, log_p) ---

unsafe fn wrap_qexp(p: c_double, scale: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::exponential::qexp_inner(p, scale, lt != 0, lp != 0)
}

pub unsafe fn do_qexp(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, wrap_qexp)
}

// --- pgeom(x, p, lower_tail, log_p) ---

unsafe fn wrap_pgeom(x: c_double, p: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::geometric::pgeom_inner(x, p, lt != 0, lp != 0)
}

pub unsafe fn do_pgeom(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, wrap_pgeom)
}

// --- qgeom(p, prob, lower_tail, log_p) ---

unsafe fn wrap_qgeom(p: c_double, prob: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::geometric::qgeom_inner(p, prob, lt != 0, lp != 0)
}

pub unsafe fn do_qgeom(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, wrap_qgeom)
}

// --- ppois(x, lambda, lower_tail, log_p) ---

unsafe fn wrap_ppois(x: c_double, lambda: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::poisson::ppois_inner(x, lambda, lt != 0, lp != 0)
}

pub unsafe fn do_ppois(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, wrap_ppois)
}

// --- qpois(p, lambda, lower_tail, log_p) ---

unsafe fn wrap_qpois(p: c_double, lambda: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::poisson::qpois_inner(p, lambda, lt != 0, lp != 0)
}

pub unsafe fn do_qpois(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, wrap_qpois)
}

// --- pt(x, df, lower_tail, log_p) ---

unsafe fn wrap_pt(x: c_double, df: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::t_dist::pt_inner(x, df, lt != 0, lp != 0)
}

pub unsafe fn do_pt(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, wrap_pt)
}

// --- qt(p, df, lower_tail, log_p) ---

unsafe fn wrap_qt(p: c_double, df: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::t_dist::qt_inner(p, df, lt != 0, lp != 0)
}

pub unsafe fn do_qt(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, wrap_qt)
}

// --- psignrank(x, n, lower_tail, log_p) ---

unsafe fn wrap_psignrank(x: c_double, n: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::signrank::psignrank_inner(x, n, lt != 0, lp != 0)
}

pub unsafe fn do_psignrank(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, wrap_psignrank)
}

// --- qsignrank(p, n, lower_tail, log_p) ---

unsafe fn wrap_qsignrank(p: c_double, n: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::signrank::qsignrank_inner(p, n, lt != 0, lp != 0)
}

pub unsafe fn do_qsignrank(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, wrap_qsignrank)
}

// ===========================================================================
// Math3 functions (3-argument)
// ===========================================================================

// --- dbeta(x, a, b, log) ---

unsafe fn wrap_dbeta(x: c_double, a: c_double, b: c_double, log: c_int) -> c_double {
    crate::nmath::dist::beta::dbeta_inner(x, a, b, log != 0)
}

pub unsafe fn do_dbeta(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_dbeta)
}

// --- dbinom(x, n, p, log) ---

unsafe fn wrap_dbinom(x: c_double, n: c_double, p: c_double, log: c_int) -> c_double {
    crate::nmath::dist::binomial::dbinom_inner(x, n, p, log != 0)
}

pub unsafe fn do_dbinom(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_dbinom)
}

// --- dcauchy(x, location, scale, log) ---

unsafe fn wrap_dcauchy(x: c_double, loc: c_double, sc: c_double, log: c_int) -> c_double {
    crate::nmath::dist::cauchy::dcauchy_inner(x, loc, sc, log != 0)
}

pub unsafe fn do_dcauchy(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_dcauchy)
}

// --- df(x, df1, df2, log) ---

unsafe fn wrap_df(x: c_double, df1: c_double, df2: c_double, log: c_int) -> c_double {
    crate::nmath::dist::f_dist::df_inner(x, df1, df2, log != 0)
}

pub unsafe fn do_df(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_df)
}

// --- dgamma(x, shape, scale, log) ---

unsafe fn wrap_dgamma(x: c_double, shape: c_double, scale: c_double, log: c_int) -> c_double {
    crate::nmath::dist::gamma::dgamma_inner(x, shape, scale, log != 0)
}

pub unsafe fn do_dgamma(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_dgamma)
}

// --- dlnorm(x, meanlog, sdlog, log) ---

unsafe fn wrap_dlnorm(x: c_double, ml: c_double, sl: c_double, log: c_int) -> c_double {
    crate::nmath::dist::lnorm::dlnorm_inner(x, ml, sl, log != 0)
}

pub unsafe fn do_dlnorm(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_dlnorm)
}

// --- dlogis(x, location, scale, log) ---

unsafe fn wrap_dlogis(x: c_double, loc: c_double, sc: c_double, log: c_int) -> c_double {
    crate::nmath::dist::logistic::dlogis_inner(x, loc, sc, log != 0)
}

pub unsafe fn do_dlogis(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_dlogis)
}

// --- dnbinom(x, size, prob, log) ---

unsafe fn wrap_dnbinom(x: c_double, size: c_double, prob: c_double, log: c_int) -> c_double {
    crate::nmath::dist::nbinom::dnbinom_inner(x, size, prob, log != 0)
}

pub unsafe fn do_dnbinom(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_dnbinom)
}

// --- dnbinom_mu(x, size, mu, log) ---

unsafe fn wrap_dnbinom_mu(x: c_double, size: c_double, mu: c_double, log: c_int) -> c_double {
    crate::nmath::dist::nbinom::dnbinom_mu_inner(x, size, mu, log != 0)
}

pub unsafe fn do_dnbinom_mu(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_dnbinom_mu)
}

// --- dnorm(x, mean, sd, log) ---

unsafe fn wrap_dnorm(x: c_double, mu: c_double, sigma: c_double, log: c_int) -> c_double {
    crate::nmath::dist::normal::dnorm4_inner(x, mu, sigma, log != 0)
}

pub unsafe fn do_dnorm(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_dnorm)
}

// --- dweibull(x, shape, scale, log) ---

unsafe fn wrap_dweibull(x: c_double, shape: c_double, scale: c_double, log: c_int) -> c_double {
    crate::nmath::dist::weibull::dweibull_inner(x, shape, scale, log != 0)
}

pub unsafe fn do_dweibull(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_dweibull)
}

// --- dunif(x, min, max, log) ---

unsafe fn wrap_dunif(x: c_double, a: c_double, b: c_double, log: c_int) -> c_double {
    crate::nmath::dist::uniform::dunif_inner(x, a, b, log != 0)
}

pub unsafe fn do_dunif(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_dunif)
}

// --- dnt(x, df, ncp, log) ---

unsafe fn wrap_dnt(x: c_double, df: c_double, ncp: c_double, log: c_int) -> c_double {
    crate::nmath::dist::nt_dist::dnt_inner(x, df, ncp, log != 0)
}

pub unsafe fn do_dnt(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_dnt)
}

// --- dnchisq(x, df, ncp, log) ---

unsafe fn wrap_dnchisq(x: c_double, df: c_double, ncp: c_double, log: c_int) -> c_double {
    crate::nmath::dist::nchisq::dnchisq_inner(x, df, ncp, log != 0)
}

pub unsafe fn do_dnchisq(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_dnchisq)
}

// --- dwilcox(x, m, n, log) ---

unsafe fn wrap_dwilcox(x: c_double, m: c_double, n: c_double, log: c_int) -> c_double {
    crate::nmath::dist::wilcox::dwilcox_inner(x, m, n, log != 0)
}

pub unsafe fn do_dwilcox(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wrap_dwilcox)
}

// ===========================================================================
// Math3_2 functions (3-argument with 2 int flags)
// ===========================================================================

// --- pbeta, qbeta ---

unsafe fn wrap_pbeta(x: c_double, a: c_double, b: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::beta::pbeta_inner(x, a, b, lt != 0, lp != 0)
}

unsafe fn wrap_qbeta(p: c_double, a: c_double, b: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::beta::qbeta_inner(p, a, b, lt != 0, lp != 0)
}

pub unsafe fn do_pbeta(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_pbeta)
}

pub unsafe fn do_qbeta(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qbeta)
}

// --- pbinom, qbinom ---

unsafe fn wrap_pbinom(x: c_double, n: c_double, p: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::binomial::pbinom_inner(x, n, p, lt != 0, lp != 0)
}

unsafe fn wrap_qbinom(p: c_double, n: c_double, pr: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::binomial::qbinom_inner(p, n, pr, lt != 0, lp != 0)
}

pub unsafe fn do_pbinom(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_pbinom)
}

pub unsafe fn do_qbinom(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qbinom)
}

// --- pcauchy, qcauchy ---

unsafe fn wrap_pcauchy(x: c_double, loc: c_double, sc: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::cauchy::pcauchy_inner(x, loc, sc, lt != 0, lp != 0)
}

unsafe fn wrap_qcauchy(p: c_double, loc: c_double, sc: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::cauchy::qcauchy_inner(p, loc, sc, lt != 0, lp != 0)
}

pub unsafe fn do_pcauchy(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_pcauchy)
}

pub unsafe fn do_qcauchy(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qcauchy)
}

// --- pf, qf ---

unsafe fn wrap_pf(x: c_double, df1: c_double, df2: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::f_dist::pf_inner(x, df1, df2, lt != 0, lp != 0)
}

unsafe fn wrap_qf(p: c_double, df1: c_double, df2: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::f_dist::qf_inner(p, df1, df2, lt != 0, lp != 0)
}

pub unsafe fn do_pf(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_pf)
}

pub unsafe fn do_qf(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qf)
}

// --- pgamma, qgamma ---

unsafe fn wrap_pgamma(
    x: c_double,
    shape: c_double,
    scale: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::gamma::pgamma_inner(x, shape, scale, lt != 0, lp != 0)
}

unsafe fn wrap_qgamma(
    p: c_double,
    shape: c_double,
    scale: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::gamma::qgamma_inner(p, shape, scale, lt != 0, lp != 0)
}

pub unsafe fn do_pgamma(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_pgamma)
}

pub unsafe fn do_qgamma(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qgamma)
}

// --- plnorm, qlnorm ---

unsafe fn wrap_plnorm(x: c_double, ml: c_double, sl: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::lnorm::plnorm_inner(x, ml, sl, lt != 0, lp != 0)
}

unsafe fn wrap_qlnorm(p: c_double, ml: c_double, sl: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::lnorm::qlnorm_inner(p, ml, sl, lt != 0, lp != 0)
}

pub unsafe fn do_plnorm(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_plnorm)
}

pub unsafe fn do_qlnorm(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qlnorm)
}

// --- plogis, qlogis ---

unsafe fn wrap_plogis(x: c_double, loc: c_double, sc: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::logistic::plogis_inner(x, loc, sc, lt != 0, lp != 0)
}

unsafe fn wrap_qlogis(p: c_double, loc: c_double, sc: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::logistic::qlogis_inner(p, loc, sc, lt != 0, lp != 0)
}

pub unsafe fn do_plogis(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_plogis)
}

pub unsafe fn do_qlogis(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qlogis)
}

// --- pnbinom, qnbinom ---

unsafe fn wrap_pnbinom(
    x: c_double,
    size: c_double,
    prob: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::nbinom::pnbinom_inner(x, size, prob, lt != 0, lp != 0)
}

unsafe fn wrap_qnbinom(
    p: c_double,
    size: c_double,
    prob: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::nbinom::qnbinom_inner(p, size, prob, lt != 0, lp != 0)
}

pub unsafe fn do_pnbinom(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_pnbinom)
}

pub unsafe fn do_qnbinom(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qnbinom)
}

// --- pnbinom_mu, qnbinom_mu ---

unsafe fn wrap_pnbinom_mu(
    x: c_double,
    size: c_double,
    mu: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::nbinom::pnbinom_mu_inner(x, size, mu, lt != 0, lp != 0)
}

unsafe fn wrap_qnbinom_mu(
    p: c_double,
    size: c_double,
    mu: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::nbinom::qnbinom_mu_inner(p, size, mu, lt != 0, lp != 0)
}

pub unsafe fn do_pnbinom_mu(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_pnbinom_mu)
}

pub unsafe fn do_qnbinom_mu(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qnbinom_mu)
}

// --- pnorm, qnorm ---

unsafe fn wrap_pnorm(x: c_double, mu: c_double, sigma: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::normal::pnorm5_inner(x, mu, sigma, lt != 0, lp != 0)
}

unsafe fn wrap_qnorm(p: c_double, mu: c_double, sigma: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::normal::qnorm5_inner(p, mu, sigma, lt != 0, lp != 0)
}

pub unsafe fn do_pnorm(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_pnorm)
}

pub unsafe fn do_qnorm(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qnorm)
}

// --- pweibull, qweibull ---

unsafe fn wrap_pweibull(
    x: c_double,
    shape: c_double,
    scale: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::weibull::pweibull_inner(x, shape, scale, lt != 0, lp != 0)
}

unsafe fn wrap_qweibull(
    p: c_double,
    shape: c_double,
    scale: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::weibull::qweibull_inner(p, shape, scale, lt != 0, lp != 0)
}

pub unsafe fn do_pweibull(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_pweibull)
}

pub unsafe fn do_qweibull(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qweibull)
}

// --- punif, qunif ---

unsafe fn wrap_punif(x: c_double, a: c_double, b: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::uniform::punif_inner(x, a, b, lt != 0, lp != 0)
}

unsafe fn wrap_qunif(p: c_double, a: c_double, b: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::uniform::qunif_inner(p, a, b, lt != 0, lp != 0)
}

pub unsafe fn do_punif(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_punif)
}

pub unsafe fn do_qunif(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qunif)
}

// --- pnt, qnt ---

unsafe fn wrap_pnt(t: c_double, df: c_double, ncp: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::nt_dist::pnt_inner(t, df, ncp, lt != 0, lp != 0)
}

unsafe fn wrap_qnt(p: c_double, df: c_double, ncp: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::nt_dist::qnt_inner(p, df, ncp, lt != 0, lp != 0)
}

pub unsafe fn do_pnt(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_pnt)
}

pub unsafe fn do_qnt(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qnt)
}

// --- pnchisq, qnchisq ---

unsafe fn wrap_pnchisq(x: c_double, df: c_double, ncp: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::nchisq::pnchisq_inner(x, df, ncp, lt != 0, lp != 0)
}

unsafe fn wrap_qnchisq(p: c_double, df: c_double, ncp: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::nchisq::qnchisq_inner(p, df, ncp, lt != 0, lp != 0)
}

pub unsafe fn do_pnchisq(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_pnchisq)
}

pub unsafe fn do_qnchisq(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qnchisq)
}

// --- pwilcox, qwilcox ---

unsafe fn wrap_pwilcox(q: c_double, m: c_double, n: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::wilcox::pwilcox_inner(q, m, n, lt != 0, lp != 0)
}

unsafe fn wrap_qwilcox(x: c_double, m: c_double, n: c_double, lt: c_int, lp: c_int) -> c_double {
    crate::nmath::dist::wilcox::qwilcox_inner(x, m, n, lt != 0, lp != 0)
}

pub unsafe fn do_pwilcox(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_pwilcox)
}

pub unsafe fn do_qwilcox(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wrap_qwilcox)
}

// ===========================================================================
// Math4 functions (4-argument)
// ===========================================================================

// --- dhyper(x, r, b, n, log) ---

unsafe fn wrap_dhyper(x: c_double, r: c_double, b: c_double, n: c_double, log: c_int) -> c_double {
    crate::nmath::dist::hypergeometric::dhyper_inner(x, r, b, n, log != 0)
}

pub unsafe fn do_dhyper(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP) -> SEXP {
    math4_1(sa, sb, sc, sd, sI, wrap_dhyper)
}

// --- dnbeta(x, a, b, ncp, log) ---

unsafe fn wrap_dnbeta(
    x: c_double,
    a: c_double,
    b: c_double,
    ncp: c_double,
    log: c_int,
) -> c_double {
    crate::nmath::dist::nbeta::dnbeta_inner(x, a, b, ncp, log != 0)
}

pub unsafe fn do_dnbeta(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP) -> SEXP {
    math4_1(sa, sb, sc, sd, sI, wrap_dnbeta)
}

// --- dnf(x, df1, df2, ncp, log) ---

unsafe fn wrap_dnf(
    x: c_double,
    df1: c_double,
    df2: c_double,
    ncp: c_double,
    log: c_int,
) -> c_double {
    crate::nmath::dist::nf_dist::dnf_inner(x, df1, df2, ncp, log != 0)
}

pub unsafe fn do_dnf(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP) -> SEXP {
    math4_1(sa, sb, sc, sd, sI, wrap_dnf)
}

// --- phyper, qhyper ---

unsafe fn wrap_phyper(
    x: c_double,
    r: c_double,
    b: c_double,
    n: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::hypergeometric::phyper_inner(x, r, b, n, lt != 0, lp != 0)
}

unsafe fn wrap_qhyper(
    p: c_double,
    r: c_double,
    b: c_double,
    n: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::hypergeometric::qhyper_inner(p, r, b, n, lt != 0, lp != 0)
}

pub unsafe fn do_phyper(
    sa: SEXP,
    sb: SEXP,
    sc: SEXP,
    sd: SEXP,
    sI: SEXP,
    sJ: SEXP,
) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, wrap_phyper)
}

pub unsafe fn do_qhyper(
    sa: SEXP,
    sb: SEXP,
    sc: SEXP,
    sd: SEXP,
    sI: SEXP,
    sJ: SEXP,
) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, wrap_qhyper)
}

// --- pnbeta, qnbeta ---

unsafe fn wrap_pnbeta(
    x: c_double,
    a: c_double,
    b: c_double,
    ncp: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::nbeta::pnbeta_inner(x, a, b, ncp, lt != 0, lp != 0)
}

unsafe fn wrap_qnbeta(
    p: c_double,
    a: c_double,
    b: c_double,
    ncp: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::nbeta::qnbeta_inner(p, a, b, ncp, lt != 0, lp != 0)
}

pub unsafe fn do_pnbeta(
    sa: SEXP,
    sb: SEXP,
    sc: SEXP,
    sd: SEXP,
    sI: SEXP,
    sJ: SEXP,
) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, wrap_pnbeta)
}

pub unsafe fn do_qnbeta(
    sa: SEXP,
    sb: SEXP,
    sc: SEXP,
    sd: SEXP,
    sI: SEXP,
    sJ: SEXP,
) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, wrap_qnbeta)
}

// --- pnf, qnf ---

unsafe fn wrap_pnf(
    x: c_double,
    df1: c_double,
    df2: c_double,
    ncp: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::nf_dist::pnf_inner(x, df1, df2, ncp, lt != 0, lp != 0)
}

unsafe fn wrap_qnf(
    p: c_double,
    df1: c_double,
    df2: c_double,
    ncp: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::nf_dist::qnf_inner(p, df1, df2, ncp, lt != 0, lp != 0)
}

pub unsafe fn do_pnf(
    sa: SEXP,
    sb: SEXP,
    sc: SEXP,
    sd: SEXP,
    sI: SEXP,
    sJ: SEXP,
) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, wrap_pnf)
}

pub unsafe fn do_qnf(
    sa: SEXP,
    sb: SEXP,
    sc: SEXP,
    sd: SEXP,
    sI: SEXP,
    sJ: SEXP,
) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, wrap_qnf)
}

// --- ptukey, qtukey ---

unsafe fn wrap_ptukey(
    q: c_double,
    nr: c_double,
    nmeans: c_double,
    df: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::tukey::ptukey_inner(q, nr, nmeans, df, lt != 0, lp != 0)
}

unsafe fn wrap_qtukey(
    p: c_double,
    nr: c_double,
    nmeans: c_double,
    df: c_double,
    lt: c_int,
    lp: c_int,
) -> c_double {
    crate::nmath::dist::tukey::qtukey_inner(p, nr, nmeans, df, lt != 0, lp != 0)
}

pub unsafe fn do_ptukey(
    sa: SEXP,
    sb: SEXP,
    sc: SEXP,
    sd: SEXP,
    sI: SEXP,
    sJ: SEXP,
) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, wrap_ptukey)
}

pub unsafe fn do_qtukey(
    sa: SEXP,
    sb: SEXP,
    sc: SEXP,
    sd: SEXP,
    sI: SEXP,
    sJ: SEXP,
) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, wrap_qtukey)
}

// ===========================================================================
// signrank_free / wilcox_free -- stubs (no-op, no caching in our port)
// ===========================================================================

pub unsafe fn stats_signrank_free(_args: SEXP) -> SEXP {
    // No-op: signrank caching not implemented
    R_NilValue()
}

pub unsafe fn stats_wilcox_free(_args: SEXP) -> SEXP {
    // No-op: wilcox caching not implemented
    R_NilValue()
}
