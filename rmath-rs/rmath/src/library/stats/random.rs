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
    unsafe {
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
}

// ---------------------------------------------------------------------------
// Helper: determine result length from length argument
// ---------------------------------------------------------------------------

unsafe fn resultLength(lengthArgument: SEXP) -> R_xlen_t {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// Helper: isNumeric check
// ---------------------------------------------------------------------------

unsafe fn isNumeric(x: SEXP) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        let t = TYPEOF(x);
        t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP
    }
}

// ---------------------------------------------------------------------------
// Helper: asReal
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
// Helper: asInteger
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
// External declarations
// ---------------------------------------------------------------------------

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    unsafe { crate::main::coerce::coerceVector(x, type_) }
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

unsafe fn duplicate(x: SEXP) -> SEXP {
    unsafe { crate::main::duplicate::duplicate(x) }
}

unsafe fn GetRNGstate() {
    unsafe { crate::main::random::GetRNGstate() }
}

unsafe fn PutRNGstate() {
    unsafe { crate::main::random::PutRNGstate() }
}

use crate::library::stats::rcont::rcont2;

// ---------------------------------------------------------------------------
// random1 -- 1-parameter random sampling
// ---------------------------------------------------------------------------

unsafe fn random1(sn: SEXP, sa: SEXP, fn_ptr: ran1, type_: SEXPTYPE) -> SEXP {
    unsafe {
        if !isNumeric(sa) {
            Rf_error(b"invalid arguments\0".as_ptr() as *const _);
            return R_NilValue();
        }
        let n = resultLength(sn);
        let x = Rf_allocVector(type_.0, n as c_int);
        if n == 0 {
            return x;
        }
        let _x_guard = protect(x);
        let na = XLENGTH(sa);

        if na < 1 {
            fillWithNAs(x, n, type_);
        } else {
            let mut naflag = false;
            let a = coerceVector(sa, SEXPTYPE::REALSXP.as_c_int());
            let _a_guard = protect(a);
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
                let mut x_real_guard = None;
                // If we switched from INTSXP, we need to re-read the data
                // For simplicity, re-allocate and fill from i0
                let x_real = if type_ == SEXPTYPE::INTSXP && i0 > 0 {
                    let xr = Rf_allocVector(SEXPTYPE::REALSXP, n as c_int);
                    x_real_guard = Some(protect(xr));
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
                drop(x_real_guard);
                return x_real;
            }
            if naflag {
                eprintln!("NAs produced");
            }
            PutRNGstate();
        }
        x
    }
}

// ---------------------------------------------------------------------------
// random2 -- 2-parameter random sampling
// ---------------------------------------------------------------------------

unsafe fn random2(sn: SEXP, sa: SEXP, sb: SEXP, fn_ptr: ran2, type_: SEXPTYPE) -> SEXP {
    unsafe {
        if !isNumeric(sa) || !isNumeric(sb) {
            Rf_error(b"invalid arguments\0".as_ptr() as *const _);
            return R_NilValue();
        }
        let n = resultLength(sn);
        let x = Rf_allocVector(type_.0, n as c_int);
        if n == 0 {
            return x;
        }
        let _x_guard = protect(x);
        let na = XLENGTH(sa);
        let nb = XLENGTH(sb);

        if na < 1 || nb < 1 {
            fillWithNAs(x, n, type_);
        } else {
            let mut naflag = false;
            let a = coerceVector(sa, SEXPTYPE::REALSXP.as_c_int());
            let _a_guard = protect(a);
            let b = coerceVector(sb, SEXPTYPE::REALSXP.as_c_int());
            let _b_guard = protect(b);
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
                let mut x_real_guard = None;
                let x_real = if type_ == SEXPTYPE::INTSXP && i0 > 0 {
                    let xr = Rf_allocVector(SEXPTYPE::REALSXP, n as c_int);
                    x_real_guard = Some(protect(xr));
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
                drop(x_real_guard);
                return x_real;
            }
            if naflag {
                eprintln!("NAs produced");
            }
            PutRNGstate();
        }
        x
    }
}

// ---------------------------------------------------------------------------
// random3 -- 3-parameter random sampling
// ---------------------------------------------------------------------------

unsafe fn random3(sn: SEXP, sa: SEXP, sb: SEXP, sc: SEXP, fn_ptr: ran3, type_: SEXPTYPE) -> SEXP {
    unsafe {
        if !isNumeric(sa) || !isNumeric(sb) || !isNumeric(sc) {
            Rf_error(b"invalid arguments\0".as_ptr() as *const _);
            return R_NilValue();
        }
        let n = resultLength(sn);
        let x = Rf_allocVector(type_.0, n as c_int);
        if n == 0 {
            return x;
        }
        let _x_guard = protect(x);
        let na = XLENGTH(sa);
        let nb = XLENGTH(sb);
        let nc = XLENGTH(sc);

        if na < 1 || nb < 1 || nc < 1 {
            fillWithNAs(x, n, type_);
        } else {
            let mut naflag = false;
            let a = coerceVector(sa, SEXPTYPE::REALSXP.as_c_int());
            let _a_guard = protect(a);
            let b = coerceVector(sb, SEXPTYPE::REALSXP.as_c_int());
            let _b_guard = protect(b);
            let c = coerceVector(sc, SEXPTYPE::REALSXP.as_c_int());
            let _c_guard = protect(c);
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
                let mut x_real_guard = None;
                let x_real = if type_ == SEXPTYPE::INTSXP && i0 > 0 {
                    let xr = Rf_allocVector(SEXPTYPE::REALSXP, n as c_int);
                    x_real_guard = Some(protect(xr));
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
                drop(x_real_guard);
                return x_real;
            }
            if naflag {
                eprintln!("NAs produced");
            }
            PutRNGstate();
        }
        x
    }
}

// ---------------------------------------------------------------------------
// 1-parameter random samplers
// ---------------------------------------------------------------------------

pub unsafe fn do_rchisq(sn: SEXP, sa: SEXP) -> SEXP {
    unsafe {
        random1(
            sn,
            sa,
            crate::nmath::dist::chisq::rchisq_inner,
            SEXPTYPE::REALSXP,
        )
    }
}

pub unsafe fn do_rexp(sn: SEXP, sa: SEXP) -> SEXP {
    unsafe {
        random1(
            sn,
            sa,
            crate::nmath::dist::exponential::rexp_inner,
            SEXPTYPE::REALSXP,
        )
    }
}

pub unsafe fn do_rgeom(sn: SEXP, sa: SEXP) -> SEXP {
    unsafe {
        random1(
            sn,
            sa,
            crate::nmath::dist::geometric::rgeom_inner,
            SEXPTYPE::INTSXP,
        )
    }
}

pub unsafe fn do_rpois(sn: SEXP, sa: SEXP) -> SEXP {
    unsafe {
        random1(
            sn,
            sa,
            crate::nmath::dist::poisson::rpois_inner,
            SEXPTYPE::INTSXP,
        )
    }
}

pub unsafe fn do_rt(sn: SEXP, sa: SEXP) -> SEXP {
    unsafe {
        random1(
            sn,
            sa,
            crate::nmath::dist::t_dist::rt_inner,
            SEXPTYPE::REALSXP,
        )
    }
}

pub unsafe fn do_rsignrank(sn: SEXP, sa: SEXP) -> SEXP {
    unsafe {
        random1(
            sn,
            sa,
            crate::nmath::dist::signrank::rsignrank_inner,
            SEXPTYPE::INTSXP,
        )
    }
}

// ---------------------------------------------------------------------------
// 2-parameter random samplers
// ---------------------------------------------------------------------------

pub unsafe fn do_rbeta(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        random2(
            sn,
            sa,
            sb,
            crate::nmath::dist::beta::rbeta_inner,
            SEXPTYPE::REALSXP,
        )
    }
}

pub unsafe fn do_rbinom(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        random2(
            sn,
            sa,
            sb,
            crate::nmath::dist::binomial::rbinom_inner,
            SEXPTYPE::INTSXP,
        )
    }
}

pub unsafe fn do_rcauchy(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        random2(
            sn,
            sa,
            sb,
            crate::nmath::dist::cauchy::rcauchy_inner,
            SEXPTYPE::REALSXP,
        )
    }
}

pub unsafe fn do_rf(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        random2(
            sn,
            sa,
            sb,
            crate::nmath::dist::f_dist::rf_inner,
            SEXPTYPE::REALSXP,
        )
    }
}

pub unsafe fn do_rgamma(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        random2(
            sn,
            sa,
            sb,
            crate::nmath::dist::gamma::rgamma_inner,
            SEXPTYPE::REALSXP,
        )
    }
}

pub unsafe fn do_rlnorm(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        random2(
            sn,
            sa,
            sb,
            crate::nmath::dist::lnorm::rlnorm_inner,
            SEXPTYPE::REALSXP,
        )
    }
}

pub unsafe fn do_rlogis(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        random2(
            sn,
            sa,
            sb,
            crate::nmath::dist::logistic::rlogis_inner,
            SEXPTYPE::REALSXP,
        )
    }
}

pub unsafe fn do_rnbinom(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        random2(
            sn,
            sa,
            sb,
            crate::nmath::dist::nbinom::rnbinom_inner,
            SEXPTYPE::INTSXP,
        )
    }
}

pub unsafe fn do_rnorm(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        random2(
            sn,
            sa,
            sb,
            crate::nmath::dist::normal::rnorm_inner,
            SEXPTYPE::REALSXP,
        )
    }
}

pub unsafe fn do_runif(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        random2(
            sn,
            sa,
            sb,
            crate::nmath::dist::uniform::runif_inner,
            SEXPTYPE::REALSXP,
        )
    }
}

pub unsafe fn do_rweibull(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        random2(
            sn,
            sa,
            sb,
            crate::nmath::dist::weibull::rweibull_inner,
            SEXPTYPE::REALSXP,
        )
    }
}

pub unsafe fn do_rwilcox(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        random2(
            sn,
            sa,
            sb,
            crate::nmath::dist::wilcox::rwilcox_inner,
            SEXPTYPE::INTSXP,
        )
    }
}

pub unsafe fn do_rnchisq(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        random2(
            sn,
            sa,
            sb,
            crate::nmath::dist::nchisq::rnchisq_inner,
            SEXPTYPE::REALSXP,
        )
    }
}

pub unsafe fn do_rnbinom_mu(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        random2(
            sn,
            sa,
            sb,
            crate::nmath::dist::nbinom::rnbinom_mu_inner,
            SEXPTYPE::INTSXP,
        )
    }
}

// ---------------------------------------------------------------------------
// 3-parameter random samplers
// ---------------------------------------------------------------------------

pub unsafe fn do_rhyper(sn: SEXP, sa: SEXP, sb: SEXP, sc: SEXP) -> SEXP {
    unsafe {
        random3(
            sn,
            sa,
            sb,
            sc,
            crate::nmath::dist::hypergeometric::rhyper_inner,
            SEXPTYPE::INTSXP,
        )
    }
}

// ---------------------------------------------------------------------------
// FixupProb -- normalize probability vector
// ---------------------------------------------------------------------------

unsafe fn FixupProb(p: *mut c_double, n: c_int) {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// do_rmultinom -- multinomial random sampling
// ---------------------------------------------------------------------------

pub unsafe fn do_rmultinom(sn: SEXP, ssize: SEXP, prob: SEXP) -> SEXP {
    unsafe {
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
        let mut prob = coerceVector(prob, SEXPTYPE::REALSXP.as_c_int());
        let k = LENGTH(prob);
        let _prob_guard = protect(prob);
        FixupProb(REAL(prob), k);

        GetRNGstate();
        let ans = allocMatrix(SEXPTYPE::INTSXP.into(), k, n);
        let _ans_guard = protect(ans);
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
            let dimnms = Rf_allocVector(SEXPTYPE::VECSXP, 2);
            let _dimnms_guard = protect(dimnms);
            SET_VECTOR_ELT(dimnms, 0, nms);
            setAttrib(ans, R_DimNamesSymbol(), dimnms);
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// Helper: allocate double/int arrays (replaces R_alloc)
// ---------------------------------------------------------------------------

unsafe fn alloc_double_array(n: usize) -> *mut c_double {
    unsafe {
        let layout = std::alloc::Layout::array::<c_double>(n).unwrap_or_else(|_| {
            std::alloc::handle_alloc_error(std::alloc::Layout::new::<c_double>())
        });
        let ptr = std::alloc::alloc(layout) as *mut c_double;
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        ptr
    }
}

unsafe fn alloc_int_array(n: usize) -> *mut c_int {
    unsafe {
        let layout = std::alloc::Layout::array::<c_int>(n)
            .unwrap_or_else(|_| std::alloc::handle_alloc_error(std::alloc::Layout::new::<c_int>()));
        let ptr = std::alloc::alloc(layout) as *mut c_int;
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        ptr
    }
}

// ---------------------------------------------------------------------------
// r2dtable -- random 2-way tables with given marginals
// ---------------------------------------------------------------------------

pub unsafe fn r2dtable(n: SEXP, r: SEXP, c: SEXP) -> SEXP {
    unsafe {
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
        let ans = Rf_allocVector(SEXPTYPE::VECSXP, n_of_samples);
        let _ans_guard = protect(ans);

        GetRNGstate();

        for i in 0..n_of_samples {
            let tmp = allocMatrix(SEXPTYPE::INTSXP.into(), nr, nc);
            let _tmp_guard = protect(tmp);
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
        }

        PutRNGstate();
        ans
    }
}

// ---------------------------------------------------------------------------
// R-level adapters -- the stock stats closures bake in argument defaults and
// ncp/prob-vs-mu/rate-vs-scale dispatch before calling the .Call entry points.
// The port has no R closures, so these adapters reproduce that front end.
// ---------------------------------------------------------------------------

unsafe fn adapter_absent(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() || x == R_MissingArg() }
}

fn missing_required(call: SEXP, name: &str) -> ! {
    crate::main::errors::errorcall_str(
        call,
        &format!("argument \"{name}\" is missing, with no default"),
    )
}

/// True when the pairlist cell's tag is `name` (named-argument dispatch:
/// the evaluator keeps call order, so `rnbinom(n, size, mu = m)` delivers
/// `mu` in the third slot). `cell` must be the argument's pairlist cons
/// cell, not its value.
unsafe fn arg_tag_is(cell: SEXP, name: &str) -> bool {
    unsafe {
        if cell.is_null() || cell == R_NilValue() {
            return false;
        }
        let tag = TAG(cell);
        if tag.is_null() || tag == R_NilValue() {
            return false;
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() {
            return false;
        }
        std::ffi::CStr::from_ptr(CHAR(pname)).to_bytes() == name.as_bytes()
    }
}

/// `x` or a ScalarReal(default) when the argument is absent; freshly
/// allocated defaults are protected via `guards` for the adapter's scope.
unsafe fn with_default(
    x: SEXP,
    default: c_double,
    guards: &mut Vec<crate::sexp::protect::ProtectGuard>,
) -> SEXP {
    unsafe {
        if adapter_absent(x) {
            let s = Rf_ScalarReal(default);
            guards.push(protect(s));
            s
        } else {
            x
        }
    }
}

/// Vectorized `1/x` (the `rexp` closure's `1/rate`), with R's Inf/NA rules.
unsafe fn reciprocal_vector(x: SEXP) -> SEXP {
    unsafe {
        let v = coerceVector(x, SEXPTYPE::REALSXP.as_c_int());
        let _v_guard = protect(v);
        let out = Rf_allocVector(SEXPTYPE::REALSXP.0, XLENGTH(v) as c_int);
        let _out_guard = protect(out);
        let src = REAL(v);
        let dst = REAL(out);
        for i in 0..XLENGTH(v) as usize {
            *dst.add(i) = 1.0 / *src.add(i);
        }
        out
    }
}

/// Elementwise `s * x` for a numeric vector (e.g. rbeta's `2 * shape1`).
unsafe fn scaled_vector(x: SEXP, s: c_double) -> SEXP {
    unsafe {
        let v = coerceVector(x, SEXPTYPE::REALSXP.as_c_int());
        let _v_guard = protect(v);
        let out = Rf_allocVector(SEXPTYPE::REALSXP.0, XLENGTH(v) as c_int);
        let _out_guard = protect(out);
        let src = REAL(v);
        let dst = REAL(out);
        for i in 0..XLENGTH(v) as usize {
            *dst.add(i) = s * *src.add(i);
        }
        out
    }
}

/// Elementwise binary op on two REALSXP results with stock recycling of the
/// shorter operand (used by the ncp composition closures).
unsafe fn vector_binop(a: SEXP, b: SEXP, f: unsafe fn(c_double, c_double) -> c_double) -> SEXP {
    unsafe {
        let na = XLENGTH(a);
        let nb = XLENGTH(b);
        let n = na.max(nb);
        let out = Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int);
        let _out_guard = protect(out);
        let pa = REAL(a);
        let pb = REAL(b);
        let dst = REAL(out);
        for i in 0..n as usize {
            *dst.add(i) = f(*pa.add(i % na as usize), *pb.add(i % nb as usize));
        }
        out
    }
}

unsafe fn div(a: c_double, b: c_double) -> c_double {
    a / b
}

unsafe fn add(a: c_double, b: c_double) -> c_double {
    a + b
}

/// Elementwise sqrt on a REALSXP vector.
unsafe fn sqrt_vector(x: SEXP) -> SEXP {
    unsafe {
        let out = Rf_allocVector(SEXPTYPE::REALSXP.0, XLENGTH(x) as c_int);
        let _out_guard = protect(out);
        let src = REAL(x);
        let dst = REAL(out);
        for i in 0..XLENGTH(x) as usize {
            *dst.add(i) = (*src.add(i)).sqrt();
        }
        out
    }
}

pub unsafe fn do_rchisq_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let df = CADR(args);
        if adapter_absent(df) {
            missing_required(call, "df");
        }
        let ncp = CADDR(args);
        if adapter_absent(ncp) {
            do_rchisq(n, df)
        } else {
            do_rnchisq(n, df, ncp)
        }
    }
}

pub unsafe fn do_rexp_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let mut guards = Vec::new();
        let rate = with_default(CADR(args), 1.0, &mut guards);
        let scale = reciprocal_vector(rate);
        do_rexp(n, scale)
    }
}

pub unsafe fn do_rgeom_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let prob = CADR(args);
        if adapter_absent(prob) {
            missing_required(call, "prob");
        }
        do_rgeom(n, prob)
    }
}

pub unsafe fn do_rpois_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let lambda = CADR(args);
        if adapter_absent(lambda) {
            missing_required(call, "lambda");
        }
        do_rpois(n, lambda)
    }
}

pub unsafe fn do_rt_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let df = CADR(args);
        if adapter_absent(df) {
            missing_required(call, "df");
        }
        let ncp = CADDR(args);
        if adapter_absent(ncp) {
            do_rt(n, df)
        } else {
            // rnorm(n, ncp)/sqrt(rchisq(n, df)/df): two full passes, in the
            // stock closure's draw order (normals first, then chisq).
            let mut guards = Vec::new();
            let one = with_default(R_NilValue(), 1.0, &mut guards);
            let z = do_rnorm(n, ncp, one);
            let _z_guard = protect(z);
            let chi = do_rchisq(n, df);
            let _chi_guard = protect(chi);
            let ratio = vector_binop(chi, df, div);
            let _ratio_guard = protect(ratio);
            let root = sqrt_vector(ratio);
            let _root_guard = protect(root);
            vector_binop(z, root, div)
        }
    }
}

pub unsafe fn do_rsignrank_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let nn = CAR(args);
        if adapter_absent(nn) {
            missing_required(call, "nn");
        }
        let n = CADR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        do_rsignrank(nn, n)
    }
}

pub unsafe fn do_rbeta_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let shape1 = CADR(args);
        if adapter_absent(shape1) {
            missing_required(call, "shape1");
        }
        let shape2 = CADDR(args);
        if adapter_absent(shape2) {
            missing_required(call, "shape2");
        }
        let ncp = CADDDR(args);
        if adapter_absent(ncp) {
            do_rbeta(n, shape1, shape2)
        } else {
            // X <- rchisq(n, 2*shape1, ncp); X/(X + rchisq(n, 2*shape2))
            let df1 = scaled_vector(shape1, 2.0);
            let _df1_guard = protect(df1);
            let x = do_rnchisq(n, df1, ncp);
            let _x_guard = protect(x);
            let df2 = scaled_vector(shape2, 2.0);
            let _df2_guard = protect(df2);
            let y = do_rchisq(n, df2);
            let _y_guard = protect(y);
            let sum = vector_binop(x, y, add);
            let _sum_guard = protect(sum);
            vector_binop(x, sum, div)
        }
    }
}

pub unsafe fn do_rbinom_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let size = CADR(args);
        if adapter_absent(size) {
            missing_required(call, "size");
        }
        let prob = CADDR(args);
        if adapter_absent(prob) {
            missing_required(call, "prob");
        }
        do_rbinom(n, size, prob)
    }
}

pub unsafe fn do_rcauchy_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let mut guards = Vec::new();
        do_rcauchy(
            n,
            with_default(CADR(args), 0.0, &mut guards),
            with_default(CADDR(args), 1.0, &mut guards),
        )
    }
}

pub unsafe fn do_rf_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let df1 = CADR(args);
        if adapter_absent(df1) {
            missing_required(call, "df1");
        }
        let df2 = CADDR(args);
        if adapter_absent(df2) {
            missing_required(call, "df2");
        }
        let ncp = CADDDR(args);
        if adapter_absent(ncp) {
            do_rf(n, df1, df2)
        } else {
            // (rchisq(n, df1, ncp)/df1) / (rchisq(n, df2)/df2)
            let num0 = do_rnchisq(n, df1, ncp);
            let _num0_guard = protect(num0);
            let num = vector_binop(num0, df1, div);
            let _num_guard = protect(num);
            let den0 = do_rchisq(n, df2);
            let _den0_guard = protect(den0);
            let den = vector_binop(den0, df2, div);
            let _den_guard = protect(den);
            vector_binop(num, den, div)
        }
    }
}

pub unsafe fn do_rgamma_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let shape = CADR(args);
        if adapter_absent(shape) {
            missing_required(call, "shape");
        }
        let mut guards = Vec::new();
        // Named rate/scale can land in either trailing slot.
        let cell3 = CDR(CDR(args));
        let cell4 = CDR(cell3);
        let mut rate = CADDR(args);
        let mut scale = CADDDR(args);
        if adapter_absent(scale) && arg_tag_is(cell3, "scale") {
            scale = rate;
            rate = R_NilValue();
        } else if adapter_absent(rate) && arg_tag_is(cell4, "rate") {
            rate = scale;
            scale = R_NilValue();
        }
        let rate_present = !adapter_absent(rate);
        let scale_present = !adapter_absent(scale);
        if rate_present && scale_present {
            // |rate * scale - 1| < 1e-15 -> warning, else error (stock)
            let rv = coerceVector(rate, SEXPTYPE::REALSXP.as_c_int());
            let _rv_guard = protect(rv);
            let sv = coerceVector(scale, SEXPTYPE::REALSXP.as_c_int());
            let _sv_guard = protect(sv);
            let r0 = *REAL(rv).add(0);
            let s0 = *REAL(sv).add(0);
            if (r0 * s0 - 1.0).abs() < 1e-15 {
                let msg = c"specify 'rate' or 'scale' but not both";
                crate::main::errors::Rf_warningcall1(call, msg.as_ptr());
            } else {
                crate::main::errors::errorcall_str(call, "specify 'rate' or 'scale' but not both");
            }
        }
        let scale_arg = if scale_present {
            scale
        } else {
            reciprocal_vector(with_default(rate, 1.0, &mut guards))
        };
        do_rgamma(n, shape, scale_arg)
    }
}

pub unsafe fn do_rlnorm_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let mut guards = Vec::new();
        do_rlnorm(
            n,
            with_default(CADR(args), 0.0, &mut guards),
            with_default(CADDR(args), 1.0, &mut guards),
        )
    }
}

pub unsafe fn do_rlogis_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let mut guards = Vec::new();
        do_rlogis(
            n,
            with_default(CADR(args), 0.0, &mut guards),
            with_default(CADDR(args), 1.0, &mut guards),
        )
    }
}

pub unsafe fn do_rnbinom_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let size = CADR(args);
        if adapter_absent(size) {
            missing_required(call, "size");
        }
        // The evaluator keeps call order, so rnbinom(n, size, mu = m)
        // delivers mu in the third slot; recognize the tag on the cells.
        let cell3 = CDR(CDR(args));
        let cell4 = CDR(cell3);
        let mut prob = CADDR(args);
        let mut mu = CADDDR(args);
        if adapter_absent(mu) && arg_tag_is(cell3, "mu") {
            mu = prob;
            prob = R_NilValue();
        } else if adapter_absent(prob) && arg_tag_is(cell4, "prob") {
            prob = mu;
            mu = R_NilValue();
        }
        if !adapter_absent(mu) {
            if !adapter_absent(prob) {
                crate::main::errors::errorcall_str(call, "'prob' and 'mu' both specified");
            }
            do_rnbinom_mu(n, size, mu)
        } else {
            if adapter_absent(prob) {
                missing_required(call, "prob");
            }
            do_rnbinom(n, size, prob)
        }
    }
}

pub unsafe fn do_rnorm_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let mut guards = Vec::new();
        do_rnorm(
            n,
            with_default(CADR(args), 0.0, &mut guards),
            with_default(CADDR(args), 1.0, &mut guards),
        )
    }
}

pub unsafe fn do_runif_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let mut guards = Vec::new();
        do_runif(
            n,
            with_default(CADR(args), 0.0, &mut guards),
            with_default(CADDR(args), 1.0, &mut guards),
        )
    }
}

pub unsafe fn do_rweibull_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = CAR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let shape = CADR(args);
        if adapter_absent(shape) {
            missing_required(call, "shape");
        }
        let mut guards = Vec::new();
        do_rweibull(n, shape, with_default(CADDR(args), 1.0, &mut guards))
    }
}

pub unsafe fn do_rwilcox_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let nn = CAR(args);
        if adapter_absent(nn) {
            missing_required(call, "nn");
        }
        let m = CADR(args);
        if adapter_absent(m) {
            missing_required(call, "m");
        }
        let n = CADDR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        do_rwilcox(nn, m, n)
    }
}

pub unsafe fn do_rhyper_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let nn = CAR(args);
        if adapter_absent(nn) {
            missing_required(call, "nn");
        }
        let m = CADR(args);
        if adapter_absent(m) {
            missing_required(call, "m");
        }
        let n = CADDR(args);
        if adapter_absent(n) {
            missing_required(call, "n");
        }
        let k = CADDDR(args);
        if adapter_absent(k) {
            missing_required(call, "k");
        }
        do_rhyper(nn, m, n, k)
    }
}
