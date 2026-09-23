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

use crate::unix::dynload::DL_FUNC;

fn as_dl<T>(f: T) -> DL_FUNC {
    Some(unsafe { std::mem::transmute_copy(&f) })
}

unsafe extern "C-unwind" fn c_rchisq(n: SEXP, a: SEXP) -> SEXP {
    unsafe { do_rchisq(n, a) }
}
unsafe extern "C-unwind" fn c_rexp(n: SEXP, a: SEXP) -> SEXP {
    unsafe { do_rexp(n, a) }
}
unsafe extern "C-unwind" fn c_rgeom(n: SEXP, a: SEXP) -> SEXP {
    unsafe { do_rgeom(n, a) }
}
unsafe extern "C-unwind" fn c_rpois(n: SEXP, a: SEXP) -> SEXP {
    unsafe { do_rpois(n, a) }
}
unsafe extern "C-unwind" fn c_rt(n: SEXP, a: SEXP) -> SEXP {
    unsafe { do_rt(n, a) }
}
unsafe extern "C-unwind" fn c_rsignrank(n: SEXP, a: SEXP) -> SEXP {
    unsafe { do_rsignrank(n, a) }
}
unsafe extern "C-unwind" fn c_rbeta(n: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe { do_rbeta(n, a, b) }
}
unsafe extern "C-unwind" fn c_rbinom(n: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe { do_rbinom(n, a, b) }
}
unsafe extern "C-unwind" fn c_rcauchy(n: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe { do_rcauchy(n, a, b) }
}
unsafe extern "C-unwind" fn c_rf(n: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe { do_rf(n, a, b) }
}
unsafe extern "C-unwind" fn c_rgamma(n: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe { do_rgamma(n, a, b) }
}
unsafe extern "C-unwind" fn c_rlnorm(n: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe { do_rlnorm(n, a, b) }
}
unsafe extern "C-unwind" fn c_rlogis(n: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe { do_rlogis(n, a, b) }
}
unsafe extern "C-unwind" fn c_rnbinom(n: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe { do_rnbinom(n, a, b) }
}
unsafe extern "C-unwind" fn c_rnorm(n: SEXP, mu: SEXP, sd: SEXP) -> SEXP {
    unsafe { do_rnorm(n, mu, sd) }
}
unsafe extern "C-unwind" fn c_runif(n: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe { do_runif(n, a, b) }
}
unsafe extern "C-unwind" fn c_rweibull(n: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe { do_rweibull(n, a, b) }
}
unsafe extern "C-unwind" fn c_rwilcox(n: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe { do_rwilcox(n, a, b) }
}
unsafe extern "C-unwind" fn c_rnchisq(n: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe { do_rnchisq(n, a, b) }
}
unsafe extern "C-unwind" fn c_rnbinom_mu(n: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe { do_rnbinom_mu(n, a, b) }
}
unsafe extern "C-unwind" fn c_rhyper(n: SEXP, a: SEXP, b: SEXP, c: SEXP) -> SEXP {
    unsafe { do_rhyper(n, a, b, c) }
}
unsafe extern "C-unwind" fn c_rmultinom(n: SEXP, size: SEXP, prob: SEXP) -> SEXP {
    unsafe { do_rmultinom(n, size, prob) }
}
unsafe extern "C-unwind" fn c_termsform(args: SEXP) -> SEXP {
    unsafe { super::filter::termsform(args) }
}
unsafe extern "C-unwind" fn c_call_dqags(args: SEXP) -> SEXP {
    unsafe { super::integrate::call_dqags(args) }
}
unsafe extern "C-unwind" fn c_call_dqagi(args: SEXP) -> SEXP {
    unsafe { super::integrate::call_dqagi(args) }
}
unsafe extern "C-unwind" fn c_modelframe(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { super::filter::modelframe(call, op, args, env) }
}
unsafe extern "C-unwind" fn c_modelmatrix(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { super::filter::modelmatrix(call, op, args, env) }
}
unsafe extern "C-unwind" fn c_cdqrls(x: SEXP, y: SEXP, tol: SEXP, chk: SEXP) -> SEXP {
    unsafe { super::lm::Cdqrls(x, y, tol, chk) }
}
unsafe extern "C-unwind" fn c_compcases(args: SEXP) -> SEXP {
    unsafe { super::complete_cases::compcases(args) }
}
unsafe extern "C-unwind" fn c_influence(mqr: SEXP, e: SEXP, stol: SEXP) -> SEXP {
    unsafe { super::influence::influence(mqr, e, stol) }
}
unsafe extern "C-unwind" fn c_cov(x: SEXP, y: SEXP, na_method: SEXP, kendall: SEXP) -> SEXP {
    unsafe { stats_call_cov(x, y, na_method, kendall) }
}
unsafe extern "C-unwind" fn c_cor(x: SEXP, y: SEXP, na_method: SEXP, kendall: SEXP) -> SEXP {
    unsafe { stats_call_cor(x, y, na_method, kendall) }
}
unsafe extern "C-unwind" fn c_cdist(x: SEXP, method: SEXP, attrs: SEXP, p: SEXP) -> SEXP {
    unsafe { super::distance::Cdist(x, method, attrs, p) }
}
unsafe extern "C-unwind" fn c_do_d(args: SEXP) -> SEXP {
    unsafe { super::deriv::do_d(args) }
}
unsafe extern "C-unwind" fn c_deriv(args: SEXP) -> SEXP {
    unsafe { super::deriv::do_deriv(args) }
}
unsafe extern "C-unwind" fn c_fft(z: SEXP, inverse: SEXP) -> SEXP {
    unsafe { super::fourier::fft(z, inverse) }
}
unsafe extern "C-unwind" fn c_mvfft(z: SEXP, inverse: SEXP) -> SEXP {
    unsafe { super::fourier::mvfft(z, inverse) }
}
unsafe extern "C-unwind" fn c_approx_test(x: SEXP, y: SEXP, method: SEXP, f: SEXP, na_rm: SEXP) -> SEXP {
    unsafe { super::approx::ApproxTest(x, y, method, f, na_rm) }
}
unsafe extern "C-unwind" fn c_approx(
    x: SEXP, y: SEXP, v: SEXP, method: SEXP, yleft: SEXP, yright: SEXP, f: SEXP, na_rm: SEXP,
) -> SEXP {
    unsafe { super::approx::Approx(x, y, v, method, yleft, yright, f, na_rm) }
}
unsafe extern "C-unwind" fn c_fisher_sim(sr: SEXP, sc: SEXP, sB: SEXP) -> SEXP {
    unsafe { super::chisqsim::Fisher_sim(sr, sc, sB) }
}
unsafe extern "C-unwind" fn c_zeroin2(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { super::zeroin::zeroin2(call, op, args, env) }
}

fn reject_var_on_factor(x: SEXP) {
    unsafe {
        if !x.is_null()
            && x != R_NilValue()
            && crate::mainutils::objects::inherits2(x, c"factor".as_ptr()) != 0
        {
            Rf_error(
                c"Calling var(x) on a factor x is defunct.\n  Use something like 'all(duplicated(x)[-1L])' to test for a constant vector."
                    .as_ptr(),
            );
        }
    }
}

/// GNU stats `C_cov` — Pearson covariance, complete/everything NA handling.
unsafe fn stats_call_cov(x: SEXP, y: SEXP, _na_method: SEXP, kendall: SEXP) -> SEXP {
    unsafe {
        reject_var_on_factor(x);
        reject_var_on_factor(y);
        if TYPEOF(kendall) == SEXPTYPE::LGLSXP
            && XLENGTH(kendall) > 0
            && *LOGICAL(kendall) != 0
        {
            Rf_error(c"Kendall covariance is not implemented".as_ptr());
        }
        let x = if TYPEOF(x) != SEXPTYPE::REALSXP {
            crate::mainutils::coerce::coerceVector(x, SEXPTYPE::REALSXP.into())
        } else {
            x
        };
        let _x = protect(x);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (n, ncx) = if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2 {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            (XLENGTH(x) as usize, 1usize)
        };
        let y_null = y.is_null() || y == R_NilValue();
        if y_null {
            let ans = if ncx == 1 {
                Rf_allocVector3(SEXPTYPE::REALSXP, 1)
            } else {
                let m = Rf_allocVector3(SEXPTYPE::REALSXP, (ncx * ncx) as i64);
                let dims = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
                *INTEGER(dims) = ncx as i32;
                *INTEGER(dims).add(1) = ncx as i32;
                crate::sexp::attrib_core::setAttrib(m, crate::sexp::attrib_core::R_DimSymbol(), dims);
                m
            };
            let _a = protect(ans);
            let xr = REAL(x);
            let ar = REAL(ans);
            for j in 0..ncx {
                for i in 0..=j {
                    let v = cov_complete_pair(xr, n, n, i, xr, n, n, j);
                    *ar.add(i + j * ncx) = v;
                    if i != j {
                        *ar.add(j + i * ncx) = v;
                    }
                }
            }
            return ans;
        }
        let y = if TYPEOF(y) != SEXPTYPE::REALSXP {
            crate::mainutils::coerce::coerceVector(y, SEXPTYPE::REALSXP.into())
        } else {
            y
        };
        let _y = protect(y);
        let ydim = crate::sexp::attrib_core::getAttrib(y, crate::sexp::attrib_core::R_DimSymbol());
        let (ny, ncy) = if TYPEOF(ydim) == SEXPTYPE::INTSXP && XLENGTH(ydim) == 2 {
            (*INTEGER(ydim) as usize, *INTEGER(ydim).add(1) as usize)
        } else {
            (XLENGTH(y) as usize, 1usize)
        };
        let nobs = n.min(ny);
        let ans = if ncx == 1 && ncy == 1 {
            Rf_allocVector3(SEXPTYPE::REALSXP, 1)
        } else {
            let m = Rf_allocVector3(SEXPTYPE::REALSXP, (ncx * ncy) as i64);
            let dims = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            *INTEGER(dims) = ncx as i32;
            *INTEGER(dims).add(1) = ncy as i32;
            crate::sexp::attrib_core::setAttrib(m, crate::sexp::attrib_core::R_DimSymbol(), dims);
            m
        };
        let _a = protect(ans);
        let xr = REAL(x);
        let yr = REAL(y);
        let ar = REAL(ans);
        for j in 0..ncy {
            for i in 0..ncx {
                *ar.add(i + j * ncx) = cov_complete_pair(xr, nobs, n, i, yr, nobs, ny, j);
            }
        }
        ans
    }
}

/// GNU stats `C_cor`. Each entry comes from one pass over the rows both
/// variables share, so a variable correlated with itself is exactly 1.
unsafe fn stats_call_cor(x: SEXP, y: SEXP, _na_method: SEXP, kendall: SEXP) -> SEXP {
    unsafe {
        reject_var_on_factor(x);
        reject_var_on_factor(y);
        let kendall = TYPEOF(kendall) == SEXPTYPE::LGLSXP
            && XLENGTH(kendall) > 0
            && *LOGICAL(kendall) != 0;
        let x = if TYPEOF(x) != SEXPTYPE::REALSXP {
            crate::mainutils::coerce::coerceVector(x, SEXPTYPE::REALSXP.into())
        } else {
            x
        };
        let _x = protect(x);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (n, ncx) = if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2 {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            (XLENGTH(x) as usize, 1usize)
        };
        let y_null = y.is_null() || y == R_NilValue();
        let (y, ny, ncy) = if y_null {
            (x, n, ncx)
        } else {
            let y = if TYPEOF(y) != SEXPTYPE::REALSXP {
                crate::mainutils::coerce::coerceVector(y, SEXPTYPE::REALSXP.into())
            } else {
                y
            };
            let _y = protect(y);
            let ydim =
                crate::sexp::attrib_core::getAttrib(y, crate::sexp::attrib_core::R_DimSymbol());
            let (ny, ncy) = if TYPEOF(ydim) == SEXPTYPE::INTSXP && XLENGTH(ydim) == 2 {
                (*INTEGER(ydim) as usize, *INTEGER(ydim).add(1) as usize)
            } else {
                (XLENGTH(y) as usize, 1usize)
            };
            (y, ny, ncy)
        };
        let nobs = n.min(ny);
        let ans = if ncx == 1 && ncy == 1 {
            Rf_allocVector3(SEXPTYPE::REALSXP, 1)
        } else {
            let m = Rf_allocVector3(SEXPTYPE::REALSXP, (ncx * ncy) as i64);
            let dims = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            *INTEGER(dims) = ncx as i32;
            *INTEGER(dims).add(1) = ncy as i32;
            crate::sexp::attrib_core::setAttrib(m, crate::sexp::attrib_core::R_DimSymbol(), dims);
            m
        };
        let _a = protect(ans);
        let xr = REAL(x);
        let yr = REAL(y);
        let ar = REAL(ans);
        for j in 0..ncy {
            for i in 0..ncx {
                *ar.add(i + j * ncx) = if kendall {
                    kendall_complete_pair(xr, nobs, n, i, yr, nobs, ny, j)
                } else {
                    cor_complete_pair(xr, nobs, n, i, yr, nobs, ny, j)
                };
            }
        }
        ans
    }
}

unsafe fn cor_complete_pair(
    x: *const f64,
    n: usize,
    ldx: usize,
    colx: usize,
    y: *const f64,
    _ny: usize,
    ldy: usize,
    coly: usize,
) -> f64 {
    unsafe {
        let mut sx = 0.0;
        let mut sy = 0.0;
        let mut count = 0usize;
        for k in 0..n {
            let xv = *x.add(k + colx * ldx);
            let yv = *y.add(k + coly * ldy);
            if xv.is_nan() || yv.is_nan() {
                continue;
            }
            sx += xv;
            sy += yv;
            count += 1;
        }
        if count < 2 {
            return crate::sexp::ffi::NA_REAL;
        }
        let mx = sx / count as f64;
        let my = sy / count as f64;
        let mut sxy = 0.0;
        let mut sxx = 0.0;
        let mut syy = 0.0;
        for k in 0..n {
            let xv = *x.add(k + colx * ldx);
            let yv = *y.add(k + coly * ldy);
            if xv.is_nan() || yv.is_nan() {
                continue;
            }
            let dx = xv - mx;
            let dy = yv - my;
            sxy += dx * dy;
            sxx += dx * dx;
            syy += dy * dy;
        }
        if sxx == 0.0 || syy == 0.0 {
            return crate::sexp::ffi::NA_REAL;
        }
        sxy / (sxx * syy).sqrt()
    }
}

/// GNU Kendall tau-b: sign products over complete pairs, divided by the
/// two self-pair counts, then clamped to [-1, 1].
unsafe fn kendall_complete_pair(
    x: *const f64,
    n: usize,
    ldx: usize,
    colx: usize,
    y: *const f64,
    _ny: usize,
    ldy: usize,
    coly: usize,
) -> f64 {
    unsafe {
        let sign = |d: f64| -> f64 { if d > 0.0 { 1.0 } else if d < 0.0 { -1.0 } else { 0.0 } };
        let mut sum = 0.0;
        let mut xsd = 0.0;
        let mut ysd = 0.0;
        for k in 0..n {
            let xk = *x.add(k + colx * ldx);
            let yk = *y.add(k + coly * ldy);
            if xk.is_nan() || yk.is_nan() {
                continue;
            }
            for n1 in 0..k {
                let x1 = *x.add(n1 + colx * ldx);
                let y1 = *y.add(n1 + coly * ldy);
                if x1.is_nan() || y1.is_nan() {
                    continue;
                }
                let xm = sign(xk - x1);
                let ym = sign(yk - y1);
                sum += xm * ym;
                xsd += xm * xm;
                ysd += ym * ym;
            }
        }
        if xsd == 0.0 || ysd == 0.0 {
            return crate::sexp::ffi::NA_REAL;
        }
        (sum / (xsd * ysd).sqrt()).clamp(-1.0, 1.0)
    }
}

unsafe fn cov_complete_pair(
    x: *const f64,
    n: usize,
    ldx: usize,
    colx: usize,
    y: *const f64,
    _ny: usize,
    ldy: usize,
    coly: usize,
) -> f64 {
    unsafe {
        let mut sx = 0.0;
        let mut sy = 0.0;
        let mut count = 0usize;
        for k in 0..n {
            let xv = *x.add(k + colx * ldx);
            let yv = *y.add(k + coly * ldy);
            if xv.is_nan() || yv.is_nan() {
                continue;
            }
            sx += xv;
            sy += yv;
            count += 1;
        }
        if count < 2 {
            return crate::sexp::ffi::NA_REAL;
        }
        let mx = sx / count as f64;
        let my = sy / count as f64;
        let mut s = 0.0;
        for k in 0..n {
            let xv = *x.add(k + colx * ldx);
            let yv = *y.add(k + coly * ldy);
            if xv.is_nan() || yv.is_nan() {
                continue;
            }
            s += (xv - mx) * (yv - my);
        }
        s / (count as f64 - 1.0)
    }
}



const RAND_CALL_NAMES: &[&str] = &[
    "C_rchisq", "C_rexp", "C_rgeom", "C_rpois", "C_rt", "C_rsignrank", "C_rbeta", "C_rbinom",
    "C_rcauchy", "C_rf", "C_rgamma", "C_rlnorm", "C_rlogis", "C_rnbinom", "C_rnorm", "C_runif",
    "C_rweibull", "C_rwilcox", "C_rnchisq", "C_rnbinom_mu", "C_rhyper", "C_rmultinom",
    "C_termsform", "C_modelframe", "C_modelmatrix", "C_updateform", "C_Cdqrls", "C_compcases", "C_influence",
    "C_cov", "C_cor", "C_Cdist", "C_hclust", "C_hcass2", "C_numeric_deriv", "C_optim", "C_optimhess",
    "C_ARIMA_transPars", "C_ARIMA_CSS", "C_ARIMA_Like", "C_ARIMA_Invtrans", "C_ARIMA_undoPars", "C_ARIMA_Gradtrans", "C_TSconv", "C_getQ0",
    "C_doD", "C_deriv", "C_fft", "C_mvfft",
    "C_ApproxTest", "C_Approx", "C_zeroin2", "C_Fisher_sim", "C_kmns", "C_call_dqags", "C_call_dqagi",
    "C_loess_raw", "C_loess_dfit", "C_loess_ifit", "C_lowesw", "C_lowesp",
    "C_kmeans_Lloyd", "C_kmeans_MacQueen", "C_Rsm", "C_pRho", "C_pKendall",
    "C_setup_starma", "C_free_starma", "C_Starma_method", "C_arma0fa",
    "C_get_s2", "C_get_resid", "C_set_trans", "C_Invtrans", "C_Dotrans", "C_Gradtrans", "C_Fexact",
];

pub fn lookup_call(name: &str) -> DL_FUNC {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "rchisq" => as_dl(c_rchisq as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "rexp" => as_dl(c_rexp as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "rgeom" => as_dl(c_rgeom as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "rpois" => as_dl(c_rpois as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "rt" => as_dl(c_rt as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "rsignrank" => as_dl(c_rsignrank as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "rbeta" => as_dl(c_rbeta as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "rbinom" => as_dl(c_rbinom as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "rcauchy" => as_dl(c_rcauchy as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "rf" => as_dl(c_rf as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "rgamma" => as_dl(c_rgamma as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "rlnorm" => as_dl(c_rlnorm as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "rlogis" => as_dl(c_rlogis as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "rnbinom" => as_dl(c_rnbinom as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "rnorm" => as_dl(c_rnorm as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "runif" => as_dl(c_runif as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "rweibull" => as_dl(c_rweibull as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "rwilcox" => as_dl(c_rwilcox as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "rnchisq" => as_dl(c_rnchisq as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "rnbinom_mu" => as_dl(c_rnbinom_mu as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "rhyper" => as_dl(c_rhyper as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP),
        "rmultinom" => as_dl(c_rmultinom as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "termsform" => as_dl(c_termsform as unsafe extern "C-unwind" fn(SEXP) -> SEXP),
        "call_dqags" => as_dl(c_call_dqags as unsafe extern "C-unwind" fn(SEXP) -> SEXP),
        "call_dqagi" => as_dl(c_call_dqagi as unsafe extern "C-unwind" fn(SEXP) -> SEXP),
        "modelframe" => {
            as_dl(c_modelframe as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP)
        }
        "modelmatrix" => {
            as_dl(c_modelmatrix as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP)
        }
        "Cdqrls" => as_dl(c_cdqrls as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP),
        "compcases" => as_dl(c_compcases as unsafe extern "C-unwind" fn(SEXP) -> SEXP),
        "influence" => as_dl(c_influence as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "cov" => as_dl(c_cov as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP),
        "cor" => as_dl(c_cor as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP),
        "Cdist" => as_dl(c_cdist as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP),
        "doD" => as_dl(c_do_d as unsafe extern "C-unwind" fn(SEXP) -> SEXP),
        "updateform" => as_dl(super::updateform::c_updateform as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "Rsm" => as_dl(super::smooth::c_rsm as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "pRho" => as_dl(super::prho::c_pRho as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "pKendall" => as_dl(super::kendall::c_pKendall as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "numeric_deriv" => as_dl(super::numeric_deriv::c_numeric_deriv as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP),
        "optim" => as_dl(super::optim::c_optim as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP),
        "optimhess" => as_dl(super::optim::c_optimhess as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP),
        "ARIMA_transPars" => as_dl(super::arima_native::c_arima_trans_pars as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "ARIMA_CSS" => as_dl(super::arima_native::c_arima_css as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP),
        "ARIMA_Like" => as_dl(super::arima_native::c_arima_like as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP),
        "ARIMA_Invtrans" => as_dl(super::arima_native::c_arima_invtrans as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "ARIMA_undoPars" => as_dl(super::arima_native::c_arima_undo_pars as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "ARIMA_Gradtrans" => as_dl(super::arima_native::c_arima_gradtrans as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "TSconv" => as_dl(super::arima_native::c_tsconv as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "getQ0" => as_dl(super::arima_native::c_get_q0 as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "deriv" => as_dl(c_deriv as unsafe extern "C-unwind" fn(SEXP) -> SEXP),
        "fft" => as_dl(c_fft as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "mvfft" => as_dl(c_mvfft as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "ApproxTest" => as_dl(
            c_approx_test as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP,
        ),
        "Approx" => as_dl(
            c_approx
                as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP,
        ),
        "zeroin2" => as_dl(
            c_zeroin2 as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP,
        ),
        "Fisher_sim" => as_dl(c_fisher_sim as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP),
        "setup_starma" => as_dl(super::starma_api::c_setup_starma as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP),
        "free_starma" => as_dl(super::starma_api::c_free_starma as unsafe extern "C-unwind" fn(SEXP) -> SEXP),
        "Starma_method" => as_dl(super::starma_api::c_starma_method as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "arma0fa" => as_dl(super::starma_api::c_arma0fa as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "get_s2" => as_dl(super::starma_api::c_get_s2 as unsafe extern "C-unwind" fn(SEXP) -> SEXP),
        "get_resid" => as_dl(super::starma_api::c_get_resid as unsafe extern "C-unwind" fn(SEXP) -> SEXP),
        "set_trans" => as_dl(super::starma_api::c_set_trans as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "Invtrans" => as_dl(super::starma_api::c_invtrans as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "Dotrans" => as_dl(super::starma_api::c_dotrans as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "Gradtrans" => as_dl(super::starma_api::c_gradtrans as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "Fexact" => as_dl(super::starma_api::c_fexact as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP),
        _ => super::distn::lookup_call(name),
    }
}

pub unsafe fn install_stats_call_symbols(env: SEXP) {
    unsafe {
        for name in RAND_CALL_NAMES
            .iter()
            .copied()
            .chain(super::distn::DISTN_CALL_NAMES.iter().copied())
        {
            let cname = std::ffi::CString::new(name).unwrap_or_default();
            crate::sexp::envir::defineVar(
                crate::sexp::symbol::Rf_install(cname.as_ptr()),
                crate::sexp::constructors::Rf_mkString(cname.as_ptr()),
                env,
            );
        }
    }
}



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
        crate::mainutils::errors::Rf_warning1(c"NAs produced".as_ptr());
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
                    crate::mainutils::errors::Rf_warning1(c"NAs produced".as_ptr());
                }
                PutRNGstate();
                drop(x_real_guard);
                return x_real;
            }
            if naflag {
                crate::mainutils::errors::Rf_warning1(c"NAs produced".as_ptr());
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
                    crate::mainutils::errors::Rf_warning1(c"NAs produced".as_ptr());
                }
                PutRNGstate();
                drop(x_real_guard);
                return x_real;
            }
            if naflag {
                crate::mainutils::errors::Rf_warning1(c"NAs produced".as_ptr());
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
                    crate::mainutils::errors::Rf_warning1(c"NAs produced".as_ptr());
                }
                PutRNGstate();
                drop(x_real_guard);
                return x_real;
            }
            if naflag {
                crate::mainutils::errors::Rf_warning1(c"NAs produced".as_ptr());
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

/// Bind `args` to `names` through GNU `matchArgs` (exact, partial, positional).
unsafe fn match_formals(call: SEXP, args: SEXP, names: &[&str]) -> Vec<SEXP> {
    unsafe { crate::mainutils::match_mod::match_formal_slots(call, args, names) }
}

unsafe fn require_slot(call: SEXP, slot: SEXP, name: &str) -> SEXP {
    if adapter_absent(slot) {
        missing_required(call, name);
    }
    slot
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
        let m = match_formals(call, args, &["n", "df", "ncp"]);
        let n = require_slot(call, m[0], "n");
        let df = require_slot(call, m[1], "df");
        if adapter_absent(m[2]) {
            do_rchisq(n, df)
        } else {
            do_rnchisq(n, df, m[2])
        }
    }
}

pub unsafe fn do_rexp_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["n", "rate"]);
        let n = require_slot(call, m[0], "n");
        let mut guards = Vec::new();
        let scale = reciprocal_vector(with_default(m[1], 1.0, &mut guards));
        do_rexp(n, scale)
    }
}

pub unsafe fn do_rgeom_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["n", "prob"]);
        do_rgeom(require_slot(call, m[0], "n"), require_slot(call, m[1], "prob"))
    }
}

pub unsafe fn do_rpois_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["n", "lambda"]);
        do_rpois(
            require_slot(call, m[0], "n"),
            require_slot(call, m[1], "lambda"),
        )
    }
}

pub unsafe fn do_rt_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["n", "df", "ncp"]);
        let n = require_slot(call, m[0], "n");
        let df = require_slot(call, m[1], "df");
        let ncp = m[2];
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
        let m = match_formals(call, args, &["nn", "n"]);
        do_rsignrank(require_slot(call, m[0], "nn"), require_slot(call, m[1], "n"))
    }
}

pub unsafe fn do_rbeta_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["n", "shape1", "shape2", "ncp"]);
        let n = require_slot(call, m[0], "n");
        let shape1 = require_slot(call, m[1], "shape1");
        let shape2 = require_slot(call, m[2], "shape2");
        let ncp = m[3];
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
        let m = match_formals(call, args, &["n", "size", "prob"]);
        do_rbinom(
            require_slot(call, m[0], "n"),
            require_slot(call, m[1], "size"),
            require_slot(call, m[2], "prob"),
        )
    }
}

pub unsafe fn do_rmultinom_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["n", "size", "prob"]);
        do_rmultinom(
            require_slot(call, m[0], "n"),
            require_slot(call, m[1], "size"),
            require_slot(call, m[2], "prob"),
        )
    }
}

pub unsafe fn do_r2dtable_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["n", "r", "c"]);
        let n = require_slot(call, m[0], "n");
        let r = require_slot(call, m[1], "r");
        let c = require_slot(call, m[2], "c");
        let n_i = coerceVector(n, SEXPTYPE::INTSXP.as_c_int());
        let _n = protect(n_i);
        let r_i = coerceVector(r, SEXPTYPE::INTSXP.as_c_int());
        let _r = protect(r_i);
        let c_i = coerceVector(c, SEXPTYPE::INTSXP.as_c_int());
        let _c = protect(c_i);
        r2dtable(n_i, r_i, c_i)
    }
}

pub unsafe fn do_rcauchy_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["n", "location", "scale"]);
        let n = require_slot(call, m[0], "n");
        let mut guards = Vec::new();
        do_rcauchy(
            n,
            with_default(m[1], 0.0, &mut guards),
            with_default(m[2], 1.0, &mut guards),
        )
    }
}

pub unsafe fn do_rf_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["n", "df1", "df2", "ncp"]);
        let n = require_slot(call, m[0], "n");
        let df1 = require_slot(call, m[1], "df1");
        let df2 = require_slot(call, m[2], "df2");
        if adapter_absent(m[3]) {
            do_rf(n, df1, df2)
        } else {
            // (rchisq(n, df1, ncp)/df1) / (rchisq(n, df2)/df2)
            let num0 = do_rnchisq(n, df1, m[3]);
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
        let m = match_formals(call, args, &["n", "shape", "rate", "scale"]);
        let n = require_slot(call, m[0], "n");
        let shape = require_slot(call, m[1], "shape");
        let rate = m[2];
        let scale = m[3];
        let mut guards = Vec::new();
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
        let m = match_formals(call, args, &["n", "meanlog", "sdlog"]);
        let n = require_slot(call, m[0], "n");
        let mut guards = Vec::new();
        do_rlnorm(
            n,
            with_default(m[1], 0.0, &mut guards),
            with_default(m[2], 1.0, &mut guards),
        )
    }
}

pub unsafe fn do_rlogis_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["n", "location", "scale"]);
        let n = require_slot(call, m[0], "n");
        let mut guards = Vec::new();
        do_rlogis(
            n,
            with_default(m[1], 0.0, &mut guards),
            with_default(m[2], 1.0, &mut guards),
        )
    }
}

pub unsafe fn do_rnbinom_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["n", "size", "prob", "mu"]);
        let n = require_slot(call, m[0], "n");
        let size = require_slot(call, m[1], "size");
        let prob = m[2];
        let mu = m[3];
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
        let m = match_formals(call, args, &["n", "mean", "sd"]);
        let n = require_slot(call, m[0], "n");
        let mut guards = Vec::new();
        do_rnorm(
            n,
            with_default(m[1], 0.0, &mut guards),
            with_default(m[2], 1.0, &mut guards),
        )
    }
}

pub unsafe fn do_runif_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["n", "min", "max"]);
        let n = require_slot(call, m[0], "n");
        let mut guards = Vec::new();
        do_runif(
            n,
            with_default(m[1], 0.0, &mut guards),
            with_default(m[2], 1.0, &mut guards),
        )
    }
}

pub unsafe fn do_rweibull_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["n", "shape", "scale"]);
        let n = require_slot(call, m[0], "n");
        let shape = require_slot(call, m[1], "shape");
        let mut guards = Vec::new();
        do_rweibull(n, shape, with_default(m[2], 1.0, &mut guards))
    }
}

pub unsafe fn do_rwilcox_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["nn", "m", "n"]);
        do_rwilcox(
            require_slot(call, m[0], "nn"),
            require_slot(call, m[1], "m"),
            require_slot(call, m[2], "n"),
        )
    }
}

pub unsafe fn do_rhyper_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = match_formals(call, args, &["nn", "m", "n", "k"]);
        do_rhyper(
            require_slot(call, m[0], "nn"),
            require_slot(call, m[1], "m"),
            require_slot(call, m[2], "n"),
            require_slot(call, m[3], "k"),
        )
    }
}
