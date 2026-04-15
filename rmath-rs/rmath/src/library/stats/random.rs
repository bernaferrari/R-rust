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
 *  Copyright (C) 1997--2025  The R Core Team
 *  Copyright (C) 2003--2016  The R Foundation
 *  Copyright (C) 1995, 1996  Robert Gentleman and Ross Ihaka
 *
 *  Ported to Rust from r-source/src/library/stats/src/random.c
 */

use std::os::raw::{c_double, c_int};
use std::ptr;

use crate::attrib_core::{R_DimNamesSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// Type aliases for random number generator function pointers
// ---------------------------------------------------------------------------

type ran1 = unsafe fn(c_double) -> c_double;
type ran2 = unsafe fn(c_double, c_double) -> c_double;
type ran3 = unsafe fn(c_double, c_double, c_double) -> c_double;

// ---------------------------------------------------------------------------
// Helper: fill vector with NAs
// ---------------------------------------------------------------------------

unsafe fn fillWithNAs(x: SEXP, n: R_xlen_t, type_: SEXPTYPE) {
    if type_ == SEXPTYPE::INTSXP {
        for i in 0..n {
            *INTEGER(x).add(i as usize) = NA_INTEGER;
        }
    } else {
        for i in 0..n {
            *REAL(x).add(i as usize) = NA_REAL;
        }
    }
    eprintln!("NAs produced");
}

// ---------------------------------------------------------------------------
// Helper: determine result length from length argument
// ---------------------------------------------------------------------------

unsafe fn resultLength(lengthArgument: SEXP) -> R_xlen_t {
    let t = TYPEOF(lengthArgument);
    if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::LGLSXP {
        Rf_error(b"invalid arguments\0".as_ptr() as *const _);
        return 0;
    }
    if XLENGTH(lengthArgument) == 1 {
        let dn = if t == SEXPTYPE::REALSXP {
            *REAL(lengthArgument)
        } else {
            let iv = *INTEGER(lengthArgument);
            if iv == NA_INTEGER {
                f64::NAN
            } else {
                iv as c_double
            }
        };
        if dn.is_nan() || dn < 0.0 {
            Rf_error(b"invalid arguments\0".as_ptr() as *const _);
            return 0;
        }
        dn as R_xlen_t
    } else {
        XLENGTH(lengthArgument)
    }
}

// ---------------------------------------------------------------------------
// Helper: isNumeric check
// ---------------------------------------------------------------------------

unsafe fn isNumeric(x: SEXP) -> bool {
    if x.is_null() {
        return false;
    }
    let t = TYPEOF(x);
    t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP
}

// ---------------------------------------------------------------------------
// Helper: asReal
// ---------------------------------------------------------------------------

unsafe fn as_real(x: SEXP) -> c_double {
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

// ---------------------------------------------------------------------------
// Helper: asInteger
// ---------------------------------------------------------------------------

unsafe fn as_integer(x: SEXP) -> c_int {
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

// ---------------------------------------------------------------------------
// External declarations
// ---------------------------------------------------------------------------

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::main::coerce::coerceVector(x, type_)
}

unsafe fn allocMatrix(sexptype: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    let ans = Rf_allocVector(sexptype, nrow * ncol);
    Rf_protect(ans);
    let dim = Rf_allocVector(SEXPTYPE::INTSXP, 2);
    Rf_protect(dim);
    *INTEGER(dim) = nrow;
    *INTEGER(dim.add(1)) = ncol;
    crate::attrib_core::setAttrib(ans, crate::attrib_core::R_DimSymbol(), dim);
    Rf_unprotect(2);
    ans
}

unsafe fn duplicate(x: SEXP) -> SEXP {
    crate::main::duplicate::duplicate(x)
}

unsafe fn GetRNGstate() {
    crate::main::random::GetRNGstate()
}

unsafe fn PutRNGstate() {
    crate::main::random::PutRNGstate()
}

unsafe extern "C" {
    fn rcont2(
        nrow: c_int,
        ncol: c_int,
        nrowt: *const c_int,
        ncolt: *const c_int,
        ntotal: c_int,
        fact: *const c_double,
        jwork: *mut c_int,
        matrix: *mut c_int,
    );
}

// ---------------------------------------------------------------------------
// random1 -- 1-parameter random sampling
// ---------------------------------------------------------------------------

unsafe fn random1(sn: SEXP, sa: SEXP, fn_ptr: ran1, type_: SEXPTYPE) -> SEXP {
    if !isNumeric(sa) {
        Rf_error(b"invalid arguments\0".as_ptr() as *const _);
        return R_NilValue();
    }
    let n = resultLength(sn);
    let x = Rf_allocVector(type_.0, n as c_int);
    if n == 0 {
        return x;
    }
    let na = XLENGTH(sa);

    if na < 1 {
        fillWithNAs(x, n, type_);
    } else {
        let mut naflag = false;
        let a = Rf_protect(coerceVector(sa, SEXPTYPE::REALSXP.0));
        let mut i0: R_xlen_t = 0;
        let mut use_type = type_;
        GetRNGstate();
        let ra = REAL(a);

        if type_ == SEXPTYPE::INTSXP {
            let ix = INTEGER(x);
            let mut i: R_xlen_t = 0;
            loop {
                if i >= n {
                    break;
                }
                let rx = fn_ptr(*ra.add((i % na) as usize));
                if ISNAN(rx) {
                    naflag = true;
                    *ix.add(i as usize) = NA_INTEGER;
                } else if rx > c_int::MAX as c_double || rx <= c_int::MIN as c_double {
                    i0 = i;
                    use_type = SEXPTYPE::REALSXP;
                    break;
                } else {
                    *ix.add(i as usize) = rx as c_int;
                }
                i += 1;
            }
        }
        if use_type == SEXPTYPE::REALSXP {
            // If we switched from INTSXP, we need to re-read the data
            // For simplicity, re-allocate and fill from i0
            let x_real = if type_ == SEXPTYPE::INTSXP && i0 > 0 {
                let xr = Rf_allocVector(SEXPTYPE::REALSXP, n as c_int);
                // Copy integer results to real
                for i in 0..i0 {
                    *REAL(xr).add(i as usize) = *INTEGER(x).add(i as usize) as c_double;
                }
                *REAL(xr).add(i0 as usize) = fn_ptr(*ra.add((i0 % na) as usize));
                xr
            } else {
                x
            };
            let rx = REAL(x_real);
            let start = if type_ == SEXPTYPE::INTSXP && i0 > 0 {
                i0 + 1
            } else {
                0
            };
            for i in start..n {
                *rx.add(i as usize) = fn_ptr(*ra.add((i % na) as usize));
                if ISNAN(*rx.add(i as usize)) {
                    naflag = true;
                }
            }
            if naflag {
                eprintln!("NAs produced");
            }
            PutRNGstate();
            Rf_unprotect(1);
            return x_real;
        }
        if naflag {
            eprintln!("NAs produced");
        }
        PutRNGstate();
        Rf_unprotect(1);
    }
    x
}

// ---------------------------------------------------------------------------
// random2 -- 2-parameter random sampling
// ---------------------------------------------------------------------------

unsafe fn random2(sn: SEXP, sa: SEXP, sb: SEXP, fn_ptr: ran2, type_: SEXPTYPE) -> SEXP {
    if !isNumeric(sa) || !isNumeric(sb) {
        Rf_error(b"invalid arguments\0".as_ptr() as *const _);
        return R_NilValue();
    }
    let n = resultLength(sn);
    let x = Rf_allocVector(type_.0, n as c_int);
    if n == 0 {
        return x;
    }
    let na = XLENGTH(sa);
    let nb = XLENGTH(sb);

    if na < 1 || nb < 1 {
        fillWithNAs(x, n, type_);
    } else {
        let mut naflag = false;
        let a = Rf_protect(coerceVector(sa, SEXPTYPE::REALSXP.0));
        let b = Rf_protect(coerceVector(sb, SEXPTYPE::REALSXP.0));
        let mut i0: R_xlen_t = 0;
        let mut use_type = type_;
        GetRNGstate();
        let ra = REAL(a);
        let rb = REAL(b);

        if type_ == SEXPTYPE::INTSXP {
            let ix = INTEGER(x);
            let mut i: R_xlen_t = 0;
            loop {
                if i >= n {
                    break;
                }
                let rx = fn_ptr(*ra.add((i % na) as usize), *rb.add((i % nb) as usize));
                if ISNAN(rx) {
                    naflag = true;
                    *ix.add(i as usize) = NA_INTEGER;
                } else if rx > c_int::MAX as c_double || rx <= c_int::MIN as c_double {
                    i0 = i;
                    use_type = SEXPTYPE::REALSXP;
                    break;
                } else {
                    *ix.add(i as usize) = rx as c_int;
                }
                i += 1;
            }
        }
        if use_type == SEXPTYPE::REALSXP {
            let x_real = if type_ == SEXPTYPE::INTSXP && i0 > 0 {
                let xr = Rf_allocVector(SEXPTYPE::REALSXP, n as c_int);
                for i in 0..i0 {
                    *REAL(xr).add(i as usize) = *INTEGER(x).add(i as usize) as c_double;
                }
                *REAL(xr).add(i0 as usize) =
                    fn_ptr(*ra.add((i0 % na) as usize), *rb.add((i0 % nb) as usize));
                xr
            } else {
                x
            };
            let rx = REAL(x_real);
            let start = if type_ == SEXPTYPE::INTSXP && i0 > 0 {
                i0 + 1
            } else {
                0
            };
            for i in start..n {
                *rx.add(i as usize) =
                    fn_ptr(*ra.add((i % na) as usize), *rb.add((i % nb) as usize));
                if ISNAN(*rx.add(i as usize)) {
                    naflag = true;
                }
            }
            if naflag {
                eprintln!("NAs produced");
            }
            PutRNGstate();
            Rf_unprotect(2);
            return x_real;
        }
        if naflag {
            eprintln!("NAs produced");
        }
        PutRNGstate();
        Rf_unprotect(2);
    }
    x
}

// ---------------------------------------------------------------------------
// random3 -- 3-parameter random sampling
// ---------------------------------------------------------------------------

unsafe fn random3(sn: SEXP, sa: SEXP, sb: SEXP, sc: SEXP, fn_ptr: ran3, type_: SEXPTYPE) -> SEXP {
    if !isNumeric(sa) || !isNumeric(sb) || !isNumeric(sc) {
        Rf_error(b"invalid arguments\0".as_ptr() as *const _);
        return R_NilValue();
    }
    let n = resultLength(sn);
    let x = Rf_allocVector(type_.0, n as c_int);
    if n == 0 {
        return x;
    }
    let na = XLENGTH(sa);
    let nb = XLENGTH(sb);
    let nc = XLENGTH(sc);

    if na < 1 || nb < 1 || nc < 1 {
        fillWithNAs(x, n, type_);
    } else {
        let mut naflag = false;
        let a = Rf_protect(coerceVector(sa, SEXPTYPE::REALSXP.0));
        let b = Rf_protect(coerceVector(sb, SEXPTYPE::REALSXP.0));
        let c = Rf_protect(coerceVector(sc, SEXPTYPE::REALSXP.0));
        let mut i0: R_xlen_t = 0;
        let mut use_type = type_;
        GetRNGstate();
        let ra = REAL(a);
        let rb = REAL(b);
        let rc = REAL(c);

        if type_ == SEXPTYPE::INTSXP {
            let ix = INTEGER(x);
            let mut i: R_xlen_t = 0;
            loop {
                if i >= n {
                    break;
                }
                let rx = fn_ptr(
                    *ra.add((i % na) as usize),
                    *rb.add((i % nb) as usize),
                    *rc.add((i % nc) as usize),
                );
                if ISNAN(rx) {
                    naflag = true;
                    *ix.add(i as usize) = NA_INTEGER;
                } else if rx > c_int::MAX as c_double || rx <= c_int::MIN as c_double {
                    i0 = i;
                    use_type = SEXPTYPE::REALSXP;
                    break;
                } else {
                    *ix.add(i as usize) = rx as c_int;
                }
                i += 1;
            }
        }
        if use_type == SEXPTYPE::REALSXP {
            let x_real = if type_ == SEXPTYPE::INTSXP && i0 > 0 {
                let xr = Rf_allocVector(SEXPTYPE::REALSXP, n as c_int);
                for i in 0..i0 {
                    *REAL(xr).add(i as usize) = *INTEGER(x).add(i as usize) as c_double;
                }
                *REAL(xr).add(i0 as usize) = fn_ptr(
                    *ra.add((i0 % na) as usize),
                    *rb.add((i0 % nb) as usize),
                    *rc.add((i0 % nc) as usize),
                );
                xr
            } else {
                x
            };
            let rx = REAL(x_real);
            let start = if type_ == SEXPTYPE::INTSXP && i0 > 0 {
                i0 + 1
            } else {
                0
            };
            for i in start..n {
                *rx.add(i as usize) = fn_ptr(
                    *ra.add((i % na) as usize),
                    *rb.add((i % nb) as usize),
                    *rc.add((i % nc) as usize),
                );
                if ISNAN(*rx.add(i as usize)) {
                    naflag = true;
                }
            }
            if naflag {
                eprintln!("NAs produced");
            }
            PutRNGstate();
            Rf_unprotect(3);
            return x_real;
        }
        if naflag {
            eprintln!("NAs produced");
        }
        PutRNGstate();
        Rf_unprotect(3);
    }
    x
}

// ---------------------------------------------------------------------------
// 1-parameter random samplers
// ---------------------------------------------------------------------------

pub unsafe fn do_rchisq(sn: SEXP, sa: SEXP) -> SEXP {
    random1(
        sn,
        sa,
        crate::nmath::dist::chisq::rchisq_inner,
        SEXPTYPE::REALSXP,
    )
}

pub unsafe fn do_rexp(sn: SEXP, sa: SEXP) -> SEXP {
    random1(
        sn,
        sa,
        crate::nmath::dist::exponential::rexp_inner,
        SEXPTYPE::REALSXP,
    )
}

pub unsafe fn do_rgeom(sn: SEXP, sa: SEXP) -> SEXP {
    random1(
        sn,
        sa,
        crate::nmath::dist::geometric::rgeom_inner,
        SEXPTYPE::INTSXP,
    )
}

pub unsafe fn do_rpois(sn: SEXP, sa: SEXP) -> SEXP {
    random1(
        sn,
        sa,
        crate::nmath::dist::poisson::rpois_inner,
        SEXPTYPE::INTSXP,
    )
}

pub unsafe fn do_rt(sn: SEXP, sa: SEXP) -> SEXP {
    random1(
        sn,
        sa,
        crate::nmath::dist::t_dist::rt_inner,
        SEXPTYPE::REALSXP,
    )
}

pub unsafe fn do_rsignrank(sn: SEXP, sa: SEXP) -> SEXP {
    random1(
        sn,
        sa,
        crate::nmath::dist::signrank::rsignrank_inner,
        SEXPTYPE::INTSXP,
    )
}

// ---------------------------------------------------------------------------
// 2-parameter random samplers
// ---------------------------------------------------------------------------

pub unsafe fn do_rbeta(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random2(
        sn,
        sa,
        sb,
        crate::nmath::dist::beta::rbeta_inner,
        SEXPTYPE::REALSXP,
    )
}

pub unsafe fn do_rbinom(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random2(
        sn,
        sa,
        sb,
        crate::nmath::dist::binomial::rbinom_inner,
        SEXPTYPE::INTSXP,
    )
}

pub unsafe fn do_rcauchy(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random2(
        sn,
        sa,
        sb,
        crate::nmath::dist::cauchy::rcauchy_inner,
        SEXPTYPE::REALSXP,
    )
}

pub unsafe fn do_rf(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random2(
        sn,
        sa,
        sb,
        crate::nmath::dist::f_dist::rf_inner,
        SEXPTYPE::REALSXP,
    )
}

pub unsafe fn do_rgamma(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random2(
        sn,
        sa,
        sb,
        crate::nmath::dist::gamma::rgamma_inner,
        SEXPTYPE::REALSXP,
    )
}

pub unsafe fn do_rlnorm(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random2(
        sn,
        sa,
        sb,
        crate::nmath::dist::lnorm::rlnorm_inner,
        SEXPTYPE::REALSXP,
    )
}

pub unsafe fn do_rlogis(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random2(
        sn,
        sa,
        sb,
        crate::nmath::dist::logistic::rlogis_inner,
        SEXPTYPE::REALSXP,
    )
}

pub unsafe fn do_rnbinom(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random2(
        sn,
        sa,
        sb,
        crate::nmath::dist::nbinom::rnbinom_inner,
        SEXPTYPE::INTSXP,
    )
}

pub unsafe fn do_rnorm(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random2(
        sn,
        sa,
        sb,
        crate::nmath::dist::normal::rnorm_inner,
        SEXPTYPE::REALSXP,
    )
}

pub unsafe fn do_runif(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random2(
        sn,
        sa,
        sb,
        crate::nmath::dist::uniform::runif_inner,
        SEXPTYPE::REALSXP,
    )
}

pub unsafe fn do_rweibull(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random2(
        sn,
        sa,
        sb,
        crate::nmath::dist::weibull::rweibull_inner,
        SEXPTYPE::REALSXP,
    )
}

pub unsafe fn do_rwilcox(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random2(
        sn,
        sa,
        sb,
        crate::nmath::dist::wilcox::rwilcox_inner,
        SEXPTYPE::INTSXP,
    )
}

pub unsafe fn do_rnchisq(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random2(
        sn,
        sa,
        sb,
        crate::nmath::dist::nchisq::rnchisq_inner,
        SEXPTYPE::REALSXP,
    )
}

pub unsafe fn do_rnbinom_mu(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random2(
        sn,
        sa,
        sb,
        crate::nmath::dist::nbinom::rnbinom_mu_inner,
        SEXPTYPE::REALSXP,
    )
}

// ---------------------------------------------------------------------------
// 3-parameter random samplers
// ---------------------------------------------------------------------------

pub unsafe fn do_rhyper(sn: SEXP, sa: SEXP, sb: SEXP, sc: SEXP) -> SEXP {
    random3(
        sn,
        sa,
        sb,
        sc,
        crate::nmath::dist::hypergeometric::rhyper_inner,
        SEXPTYPE::INTSXP,
    )
}

// ---------------------------------------------------------------------------
// FixupProb -- normalize probability vector
// ---------------------------------------------------------------------------

unsafe fn FixupProb(p: *mut c_double, n: c_int) {
    let mut sum = 0.0;
    let mut npos = 0;
    for i in 0..n {
        if !R_FINITE(*p.add(i as usize)) {
            Rf_error(b"NA in probability vector\0".as_ptr() as *const _);
            return;
        }
        if *p.add(i as usize) < 0.0 {
            Rf_error(b"negative probability\0".as_ptr() as *const _);
            return;
        }
        if *p.add(i as usize) > 0.0 {
            npos += 1;
            sum += *p.add(i as usize);
        }
    }
    if npos == 0 {
        Rf_error(b"no positive probabilities\0".as_ptr() as *const _);
        return;
    }
    for i in 0..n {
        *p.add(i as usize) /= sum;
    }
}

// ---------------------------------------------------------------------------
// do_rmultinom -- multinomial random sampling
// ---------------------------------------------------------------------------

pub unsafe fn do_rmultinom(sn: SEXP, ssize: SEXP, prob: SEXP) -> SEXP {
    let n = as_integer(sn);
    let size = as_integer(ssize);
    if n == NA_INTEGER || n < 0 {
        Rf_error(b"invalid first argument 'n'\0".as_ptr() as *const _);
        return R_NilValue();
    }
    if size == NA_INTEGER || size < 0 {
        Rf_error(b"invalid second argument 'size'\0".as_ptr() as *const _);
        return R_NilValue();
    }
    let mut prob = coerceVector(prob, SEXPTYPE::REALSXP.0);
    let k = LENGTH(prob);
    Rf_protect(prob);
    FixupProb(REAL(prob), k);

    GetRNGstate();
    let ans = Rf_protect(allocMatrix(SEXPTYPE::INTSXP, k, n));
    let mut rn_buf: Vec<f64> = vec![0.0; k as usize];
    for i in 0..n as R_xlen_t {
        let ik = i * k as R_xlen_t;
        crate::nmath::dist::multinom::rmultinom_inner(
            size,
            std::slice::from_raw_parts(REAL(prob), k as usize),
            &mut rn_buf,
        );
        // Copy f64 results to integer output
        for j in 0..k as usize {
            *INTEGER(ans).add((ik + j as R_xlen_t) as usize) = rn_buf[j] as c_int;
        }
    }
    PutRNGstate();

    let nms = getAttrib(prob, R_NamesSymbol());
    if Rf_isNull(nms) == 0 {
        let dimnms = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 2));
        SET_VECTOR_ELT(dimnms, 0, nms);
        setAttrib(ans, R_DimNamesSymbol(), dimnms);
        Rf_unprotect(1);
    }
    Rf_unprotect(2);
    ans
}

// ---------------------------------------------------------------------------
// Helper: allocate double/int arrays (replaces R_alloc)
// ---------------------------------------------------------------------------

unsafe fn alloc_double_array(n: usize) -> *mut c_double {
    let layout = std::alloc::Layout::array::<c_double>(n)
        .unwrap_or_else(|_| std::alloc::handle_alloc_error(std::alloc::Layout::new::<c_double>()));
    let ptr = std::alloc::alloc(layout) as *mut c_double;
    if ptr.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    ptr
}

unsafe fn alloc_int_array(n: usize) -> *mut c_int {
    let layout = std::alloc::Layout::array::<c_int>(n)
        .unwrap_or_else(|_| std::alloc::handle_alloc_error(std::alloc::Layout::new::<c_int>()));
    let ptr = std::alloc::alloc(layout) as *mut c_int;
    if ptr.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    ptr
}

// ---------------------------------------------------------------------------
// r2dtable -- random 2-way tables with given marginals
// ---------------------------------------------------------------------------

pub unsafe fn r2dtable(n: SEXP, r: SEXP, c: SEXP) -> SEXP {
    let nr = LENGTH(r);
    let nc = LENGTH(c);

    if TYPEOF(n) != SEXPTYPE::INTSXP
        || LENGTH(n) == 0
        || TYPEOF(r) != SEXPTYPE::INTSXP
        || nr <= 1
        || TYPEOF(c) != SEXPTYPE::INTSXP
        || nc <= 1
    {
        Rf_error(b"invalid arguments\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let n_of_samples = *INTEGER(n);
    let row_sums = INTEGER(r);
    let col_sums = INTEGER(c);

    // Compute total cases as sum of row sums
    let mut n_of_cases: c_int = 0;
    for i in 0..nr {
        n_of_cases += *row_sums.add(i as usize);
    }

    // Log-factorials
    let fact = alloc_double_array((n_of_cases + 1) as usize);
    *fact.add(0) = 0.0;
    for i in 1..=n_of_cases {
        *fact.add(i as usize) = crate::nmath::special::gamma::lgammafn((i + 1) as c_double);
    }

    let jwork = alloc_int_array(nc as usize);
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, n_of_samples));

    GetRNGstate();

    for i in 0..n_of_samples {
        let tmp = Rf_protect(allocMatrix(SEXPTYPE::INTSXP, nr, nc));
        rcont2(
            nr,
            nc,
            row_sums,
            col_sums,
            n_of_cases,
            fact,
            jwork,
            INTEGER(tmp),
        );
        SET_VECTOR_ELT(ans, i as R_xlen_t, tmp);
        Rf_unprotect(1);
    }

    PutRNGstate();
    Rf_unprotect(1);
    ans
}
