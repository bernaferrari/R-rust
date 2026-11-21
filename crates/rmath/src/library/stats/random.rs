/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1997--2025  The R Core Team
 *  Copyright (C) 2003--2016  The R Foundation
 *  Copyright (C) 1995, 1996  Robert Gentleman and Ross Ihaka
 *
 *  Ported to Rust from r-source/src/library/stats/src/random.c
 */

use std::os::raw::{c_double, c_int};

use crate::attrib_core::{R_DimNamesSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::main::coerce::coerceVector;
use crate::main::errors::Rf_error;
use crate::main::random::{GetRNGstate, PutRNGstate};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::object::SexpMut;
use crate::sexp::protect::*;

// Registered .Call entry points keep the raw ABI edge: the trampoline macros
// write the unsafe declaration and dispatch once for every handler instead of
// repeating it per routine. `call_trampoline!` forwards to a safe handler that
// owns argument validation under the held runtime lock; `call_trampoline_raw!`
// forwards to a `pub fn` handler that still owns the raw SEXP contract.
macro_rules! call_trampoline {
    ($name:ident, ($($arg:ident),*) => $handler:expr) => {
        // SAFETY: this is a registered .Call routine; the interpreter holds the
        // runtime lock across dispatch and the callee owns argument validation.
        unsafe extern "C-unwind" fn $name($($arg: SEXP),*) -> SEXP {
            $handler
        }
    };
}
macro_rules! call_trampoline_raw {
    ($name:ident, ($($arg:ident),*) => $handler:expr) => {
        // SAFETY: this is a registered .Call routine; the interpreter holds the
        // runtime lock across dispatch and the callee owns the raw SEXP
        // contract for these arguments.
        unsafe extern "C-unwind" fn $name($($arg: SEXP),*) -> SEXP {
            unsafe { $handler }
        }
    };
}

call_trampoline!(c_rchisq, (n, a) => do_rchisq(n, a));
call_trampoline!(c_rexp, (n, a) => do_rexp(n, a));
call_trampoline!(c_rgeom, (n, a) => do_rgeom(n, a));
call_trampoline!(c_rpois, (n, a) => do_rpois(n, a));
call_trampoline!(c_rt, (n, a) => do_rt(n, a));
call_trampoline!(c_rsignrank, (n, a) => do_rsignrank(n, a));
call_trampoline!(c_rbeta, (n, a, b) => do_rbeta(n, a, b));
call_trampoline!(c_rbinom, (n, a, b) => do_rbinom(n, a, b));
call_trampoline!(c_rcauchy, (n, a, b) => do_rcauchy(n, a, b));
call_trampoline!(c_rf, (n, a, b) => do_rf(n, a, b));
call_trampoline!(c_rgamma, (n, a, b) => do_rgamma(n, a, b));
call_trampoline!(c_rlnorm, (n, a, b) => do_rlnorm(n, a, b));
call_trampoline!(c_rlogis, (n, a, b) => do_rlogis(n, a, b));
call_trampoline!(c_rnbinom, (n, a, b) => do_rnbinom(n, a, b));
call_trampoline!(c_rnorm, (n, mu, sd) => do_rnorm(n, mu, sd));
call_trampoline!(c_runif, (n, a, b) => do_runif(n, a, b));
call_trampoline!(c_rweibull, (n, a, b) => do_rweibull(n, a, b));
call_trampoline!(c_rwilcox, (n, a, b) => do_rwilcox(n, a, b));
call_trampoline!(c_rnchisq, (n, a, b) => do_rnchisq(n, a, b));
call_trampoline!(c_rnbinom_mu, (n, a, b) => do_rnbinom_mu(n, a, b));
call_trampoline!(c_rhyper, (n, a, b, c) => do_rhyper(n, a, b, c));
call_trampoline!(c_rmultinom, (n, size, prob) => do_rmultinom(n, size, prob));
call_trampoline!(c_r2dtable, (n, r, c) => { let n_i = coerceVector(n, SEXPTYPE::INTSXP.as_c_int()); let _n = protect(n_i); let r_i = coerceVector(r, SEXPTYPE::INTSXP.as_c_int()); let _r = protect(r_i); let c_i = coerceVector(c, SEXPTYPE::INTSXP.as_c_int()); let _c = protect(c_i); r2dtable(n_i, r_i, c_i) });
call_trampoline!(c_termsform, (args) => super::filter::termsform(args));
call_trampoline!(c_call_dqags, (args) => super::integrate::call_dqags(args));
call_trampoline!(c_call_dqagi, (args) => super::integrate::call_dqagi(args));
call_trampoline!(c_modelframe, (call, op, args, env) => super::filter::modelframe(call, op, args, env));
call_trampoline!(c_modelmatrix, (call, op, args, env) => super::filter::modelmatrix(call, op, args, env));
call_trampoline!(c_cdqrls, (x, y, tol, chk) => super::lm::Cdqrls(x, y, tol, chk));
call_trampoline!(c_compcases, (args) => super::complete_cases::compcases(args));
call_trampoline!(c_influence, (mqr, e, stol) => super::influence::influence(mqr, e, stol));
call_trampoline_raw!(c_cov, (x, y, na_method, kendall) => super::correlation::cov(x, y, na_method, kendall));
call_trampoline_raw!(c_cor, (x, y, na_method, kendall) => super::correlation::cor(x, y, na_method, kendall));
call_trampoline!(c_chisq_sim, (sr, sc, b, e) => super::chisqsim::chisq_sim(sr, sc, b, e));
call_trampoline!(c_nextn, (n, f) => super::fourier::nextn(n, f));
call_trampoline!(c_cdist, (x, method, attrs, p) => super::distance::Cdist(x, method, attrs, p));
call_trampoline!(c_do_d, (args) => super::deriv::do_d(args));
call_trampoline!(c_deriv, (args) => super::deriv::do_deriv(args));
call_trampoline!(c_fft, (z, inverse) => super::fourier::fft(z, inverse));
call_trampoline!(c_mvfft, (z, inverse) => super::fourier::mvfft(z, inverse));
call_trampoline!(c_approx_test, (x, y, method, f, na_rm) => super::approx::ApproxTest(x, y, method, f, na_rm));
call_trampoline!(c_approx, (x, y, v, method, yleft, yright, f, na_rm) => super::approx::Approx(x, y, v, method, yleft, yright, f, na_rm));
call_trampoline!(c_fisher_sim, (sr, sc, sB) => super::chisqsim::Fisher_sim(sr, sc, sB));
call_trampoline_raw!(c_zeroin2, (call, op, args, env) => super::zeroin::zeroin2(call, op, args, env));
call_trampoline_raw!(c_do_fmin, (call, op, args, env) => super::zeroin::do_fmin(call, op, args, env));

pub(super) fn reject_var_on_factor(x: SEXP) {
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

const RAND_CALL_NAMES: &[&str] = &[
    "C_rchisq",
    "C_rexp",
    "C_rgeom",
    "C_rpois",
    "C_rt",
    "C_rsignrank",
    "C_rbeta",
    "C_rbinom",
    "C_rcauchy",
    "C_rf",
    "C_rgamma",
    "C_rlnorm",
    "C_rlogis",
    "C_rnbinom",
    "C_rnorm",
    "C_runif",
    "C_rweibull",
    "C_rwilcox",
    "C_rnchisq",
    "C_rnbinom_mu",
    "C_rhyper",
    "C_rmultinom",
    "C_r2dtable",
    "C_termsform",
    "C_modelframe",
    "C_modelmatrix",
    "C_updateform",
    "C_Cdqrls",
    "C_compcases",
    "C_influence",
    "C_cov",
    "C_cor",
    "C_Cdist",
    "C_hclust",
    "C_hcass2",
    "C_rbart",
    "C_bvalus",
    "C_numeric_deriv",
    "C_optim",
    "C_optimhess",
    "C_nlm",
    "C_ARIMA_transPars",
    "C_ARIMA_CSS",
    "C_ARIMA_Like",
    "C_ARIMA_Invtrans",
    "C_ARIMA_undoPars",
    "C_ARIMA_Gradtrans",
    "C_TSconv",
    "C_getQ0",
    "C_doD",
    "C_deriv",
    "C_fft",
    "C_mvfft",
    "C_ApproxTest",
    "C_Approx",
    "C_zeroin2",
    "C_do_fmin",
    "C_Fisher_sim",
    "C_kmns",
    "C_eureka",
    "C_multi_yw",
    "C_call_dqags",
    "C_call_dqagi",
    "C_loess_raw",
    "C_loess_dfit",
    "C_loess_ifit",
    "C_loess_ise",
    "C_loess_dfitse",
    "C_lowesw",
    "C_lowesp",
    "C_lowess",
    "C_kmeans_Lloyd",
    "C_kmeans_MacQueen",
    "C_Rsm",
    "C_acf",
    "C_pacf1",
    "C_SWilk",
    "C_nls_iter",
    "C_tukeyline",
    "C_pRho",
    "C_pKendall",
    "C_ksmooth",
    "C_rfilter",
    "C_cfilter",
    "C_arma0_kfore",
    "C_KalmanFore",
    "C_KalmanLike",
    "C_KalmanSmooth",
    "C_psmirnov_exact",
    "C_rWishart",
    "C_ar2ma",
    "C_dpermdist1",
    "C_dpermdist2",
    "C_setup_starma",
    "C_free_starma",
    "C_Starma_method",
    "C_arma0fa",
    "C_get_s2",
    "C_get_resid",
    "C_set_trans",
    "C_Invtrans",
    "C_Dotrans",
    "C_Gradtrans",
    "C_Fexact",
    "C_SplineCoef",
    "C_SplineEval",
    "C_logit_link",
    "C_logit_linkinv",
    "C_logit_mu_eta",
    "C_binomial_dev_resids",
    "C_DoubleCentre",
    "C_cutree",
    "C_monoFC_m",
    "C_runmed",
    "C_chisq_sim",
    "C_nextn",
    "C_BinDist",
    "C_bw_den",
    "C_bw_den_binned",
    "C_bw_ucv",
    "C_bw_bcv",
    "C_bw_phi4",
    "C_bw_phi6",
    "C_HoltWinters",
    "C_stl",
    "C_setsmu",
    "C_supsmu",
    "C_setppr",
    "C_smart",
    "C_pppred",
    "C_isoreg",
    "C_pAnsari",
    "C_qAnsari",
    "C_intgrt_vec",
    "C_pp_sum",
    "C_ARMAtoMA",
    "C_Burg",
    "C_LogLin",
    "C_port_ivset",
    "C_port_nlminb",
    "C_Smirnov_sim",
    "C_pkolmogorov_two_limit",
    "C_pkolmogorov_two_exact",
];

call_trampoline!(c_bindist, (sx, sw, slo, shi, sn) => super::massdist::BinDist(sx, sw, slo, shi, sn));

call_trampoline_raw!(c_isoreg, (y) => super::isoreg::isoreg(y));

call_trampoline!(c_pansari, (q, m, n) => super::ansari::pAnsari(q, m, n));

call_trampoline!(c_qansari, (p, m, n) => super::ansari::qAnsari(p, m, n));

call_trampoline!(c_intgrt_vec, (x, xi, lag) => super::ppsum::intgrt_vec(x, xi, lag));

call_trampoline!(c_pp_sum, (u, l) => super::ppsum::pp_sum(u, l));

call_trampoline!(c_armatoma, (ar, ma, lag_max) => super::filter::ARMAtoMA(ar, ma, lag_max));

call_trampoline!(c_burg, (x, order) => super::burg::Burg(x, order));

call_trampoline!(c_loglin, (dtab, conf, table, start, nmar, eps, iter) => super::loglin::LogLin(dtab, conf, table, start, nmar, eps, iter));

call_trampoline!(c_port_ivset, (kind, iv, v) => super::port::port_ivset(kind, iv, v));

#[allow(clippy::too_many_arguments)]
call_trampoline!(c_port_nlminb, (fn_expr, gr_expr, hs_expr, rho, lowerb, upperb, d, iv, v) => super::port::port_nlminb(fn_expr, gr_expr, hs_expr, rho, lowerb, upperb, d, iv, v));

call_trampoline!(c_smirnov_sim, (sr, sc, sb, twosided) => super::ks::Smirnov_sim(sr, sc, sb, twosided));

call_trampoline!(c_pkolmogorov_two_limit, (sq, lower, tol) => super::ks::pkolmogorov_two_limit(sq, lower, tol));

call_trampoline!(c_pkolmogorov_two_exact, (sq, sn) => super::ks::pkolmogorov_two_exact(sq, sn));

call_trampoline!(c_swilk, (x) => super::swilk::SWilk(x));

/// Routines whose real signature is `.External2` `(call, op, args, env)`.
/// `.External` must not transmute a `.Call` pointer from `lookup_call`.
pub(crate) fn lookup_external(
    name: &str,
) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "zeroin2" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_zeroin2,
            crate::mainutils::native_routines::PayloadArity::Fixed(7),
        )),
        "do_fmin" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_do_fmin,
            crate::mainutils::native_routines::PayloadArity::Fixed(4),
        )),
        "termsform" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_termsform,
            crate::mainutils::native_routines::PayloadArity::Fixed(5),
        )),
        "call_dqags" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_call_dqags,
            crate::mainutils::native_routines::PayloadArity::Fixed(7),
        )),
        "call_dqagi" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_call_dqagi,
            crate::mainutils::native_routines::PayloadArity::Fixed(7),
        )),
        "compcases" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_compcases,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "doD" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_do_d,
            crate::mainutils::native_routines::PayloadArity::Fixed(2),
        )),
        "deriv" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_deriv,
            crate::mainutils::native_routines::PayloadArity::Fixed(5),
        )),
        "optim" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            super::optim::c_optim,
            crate::mainutils::native_routines::PayloadArity::Fixed(7),
        )),
        "nlm" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            super::zeroin::c_nlm,
            crate::mainutils::native_routines::PayloadArity::Fixed(11),
        )),
        "optimhess" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            super::optim::c_optimhess,
            crate::mainutils::native_routines::PayloadArity::Fixed(4),
        )),
        "modelframe" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_modelframe,
            crate::mainutils::native_routines::PayloadArity::Fixed(8),
        )),
        "modelmatrix" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_modelmatrix,
            crate::mainutils::native_routines::PayloadArity::Fixed(2),
        )),
        "signrank_free" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            super::distn::c_signrank_free,
            crate::mainutils::native_routines::PayloadArity::Fixed(0),
        )),
        "wilcox_free" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            super::distn::c_wilcox_free,
            crate::mainutils::native_routines::PayloadArity::Fixed(0),
        )),
        _ => None,
    }
}

/// Recorded `.Call` arity. `None` means this name is not a stats `.Call` routine.
pub fn call_arity(name: &str) -> Option<usize> {
    match lookup_call(name)? {
        crate::mainutils::native_routines::NativeRoutine::Call(function) => Some(function.arity()),
        _ => None,
    }
}

pub(crate) fn lookup_call(name: &str) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "rchisq" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_rchisq),
        )),
        "rexp" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_rexp),
        )),
        "rgeom" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_rgeom),
        )),
        "rpois" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_rpois),
        )),
        "rt" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_rt),
        )),
        "rsignrank" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_rsignrank),
        )),
        "rbeta" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_rbeta),
        )),
        "rbinom" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_rbinom),
        )),
        "rcauchy" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_rcauchy),
        )),
        "rf" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_rf),
        )),
        "rgamma" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_rgamma),
        )),
        "rlnorm" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_rlnorm),
        )),
        "rlogis" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_rlogis),
        )),
        "rnbinom" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_rnbinom),
        )),
        "rnorm" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_rnorm),
        )),
        "runif" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_runif),
        )),
        "rweibull" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_rweibull),
        )),
        "rwilcox" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_rwilcox),
        )),
        "rnchisq" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_rnchisq),
        )),
        "rnbinom_mu" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_rnbinom_mu),
        )),
        "rhyper" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_rhyper),
        )),
        "rmultinom" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_rmultinom),
        )),
        "r2dtable" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_r2dtable),
        )),
        "rWishart" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(
                crate::library::stats::rwishart::c_rWishart,
            ),
        )),
        "isoreg" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(c_isoreg),
        )),
        "pAnsari" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_pansari),
        )),
        "qAnsari" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_qansari),
        )),
        "intgrt_vec" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_intgrt_vec),
        )),
        "pp_sum" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_pp_sum),
        )),
        "ARMAtoMA" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_armatoma),
        )),
        "Burg" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_burg),
        )),
        "LogLin" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args7(c_loglin),
        )),
        "port_ivset" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_port_ivset),
        )),
        "port_nlminb" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args9(c_port_nlminb),
        )),
        "Smirnov_sim" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_smirnov_sim),
        )),
        "pkolmogorov_two_limit" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_pkolmogorov_two_limit),
        )),
        "pkolmogorov_two_exact" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_pkolmogorov_two_exact),
        )),
        "Cdqrls" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_cdqrls),
        )),
        "influence" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_influence),
        )),
        "cov" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_cov),
        )),
        "cor" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_cor),
        )),
        "Cdist" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_cdist),
        )),
        "updateform" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::updateform::c_updateform),
        )),
        "Rsm" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(super::smooth::c_rsm),
        )),
        "lowess" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(super::lowess::c_lowess),
        )),
        "KalmanFore" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(super::filter::c_kalman_fore),
        )),
        "KalmanLike" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(super::filter::c_kalman_like),
        )),
        "KalmanSmooth" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(super::filter::c_kalman_smooth),
        )),
        "acf" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(super::filter::c_acf),
        )),
        "pacf1" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::filter::c_pacf1),
        )),
        "SWilk" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(c_swilk),
        )),
        "nls_iter" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(super::nls_iter::c_nls_iter),
        )),
        "bw_den" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::bandwidths::c_bw_den),
        )),
        "bw_den_binned" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(
                super::bandwidths::c_bw_den_binned,
            ),
        )),
        "bw_ucv" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(super::bandwidths::c_bw_ucv),
        )),
        "bw_bcv" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(super::bandwidths::c_bw_bcv),
        )),
        "bw_phi4" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(super::bandwidths::c_bw_phi4),
        )),
        "bw_phi6" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(super::bandwidths::c_bw_phi6),
        )),
        "BinDist" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_bindist),
        )),
        "runmed" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args6(super::srunmed::c_runmed),
        )),
        "tukeyline" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(super::line::c_tukeyline),
        )),
        "nextn" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_nextn),
        )),
        "chisq_sim" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_chisq_sim),
        )),
        "ksmooth" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(super::ksmooth::c_ksmooth),
        )),
        "rfilter" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(super::filter::c_rfilter),
        )),
        "arma0_kfore" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(super::starma_api::c_arma0_kfore),
        )),
        "psmirnov_exact" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args6(
                crate::mainutils::essentials::c_psmirnov_exact,
            ),
        )),
        "cfilter" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(super::filter::c_cfilter),
        )),
        "pRho" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(super::prho::c_pRho),
        )),
        "pKendall" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::kendall::c_pKendall),
        )),
        "ar2ma" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::filter::c_ar2ma),
        )),
        "dpermdist1" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(super::permdist::c_dpermdist1),
        )),
        "dpermdist2" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::permdist::c_dpermdist2),
        )),
        "numeric_deriv" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args6(
                super::numeric_deriv::c_numeric_deriv,
            ),
        )),
        "ARIMA_transPars" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(
                super::arima_native::c_arima_trans_pars,
            ),
        )),
        "ARIMA_CSS" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args6(super::arima_native::c_arima_css),
        )),
        "ARIMA_Like" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(
                super::arima_native::c_arima_like,
            ),
        )),
        "ARIMA_Invtrans" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(
                super::arima_native::c_arima_invtrans,
            ),
        )),
        "ARIMA_undoPars" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(
                super::arima_native::c_arima_undo_pars,
            ),
        )),
        "ARIMA_Gradtrans" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(
                super::arima_native::c_arima_gradtrans,
            ),
        )),
        "TSconv" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::arima_native::c_tsconv),
        )),
        "getQ0" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::arima_native::c_get_q0),
        )),
        "fft" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_fft),
        )),
        "mvfft" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_mvfft),
        )),
        "ApproxTest" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_approx_test),
        )),
        "Approx" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args8(c_approx),
        )),
        "zeroin2" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_zeroin2,
            crate::mainutils::native_routines::PayloadArity::Fixed(7),
        )),
        "do_fmin" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_do_fmin,
            crate::mainutils::native_routines::PayloadArity::Fixed(4),
        )),
        "Fisher_sim" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_fisher_sim),
        )),
        "setup_starma" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args8(
                super::starma_api::c_setup_starma,
            ),
        )),
        "free_starma" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(super::starma_api::c_free_starma),
        )),
        "Starma_method" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(
                super::starma_api::c_starma_method,
            ),
        )),
        "arma0fa" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::starma_api::c_arma0fa),
        )),
        "get_s2" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(super::starma_api::c_get_s2),
        )),
        "get_resid" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(super::starma_api::c_get_resid),
        )),
        "set_trans" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::starma_api::c_set_trans),
        )),
        "Invtrans" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::starma_api::c_invtrans),
        )),
        "Dotrans" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::starma_api::c_dotrans),
        )),
        "Gradtrans" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::starma_api::c_gradtrans),
        )),
        "Fexact" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(super::starma_api::c_fexact),
        )),
        "SplineCoef" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(super::splines::c_spline_coef),
        )),
        "SplineEval" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::splines::c_spline_eval),
        )),
        "logit_link" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(super::family::c_logit_link),
        )),
        "logit_linkinv" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(super::family::c_logit_linkinv),
        )),
        "logit_mu_eta" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(super::family::c_logit_mu_eta),
        )),
        "binomial_dev_resids" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(
                super::family::c_binomial_dev_resids,
            ),
        )),
        "DoubleCentre" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(super::dblcen::c_double_centre),
        )),
        "cutree" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::hclust_utils::c_cutree),
        )),
        "monoFC_m" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(super::hclust_utils::c_mono_fc_m),
        )),
        _ => super::distn::lookup_call(name),
    }
}

pub fn install_stats_call_symbols(env: SEXP) {
    // SAFETY: registration runs under the held runtime lock; `env` is a live
    // environment and each symbol/string allocation is a fresh, valid SEXP.
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
// Helper: determine result length from length argument
// ---------------------------------------------------------------------------

fn checked<T>(result: crate::sexp::object::SexpResult<T>) -> T {
    result.unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()))
}

fn result_length(owner: crate::sexp::owner::OwnerToken<'_>, length_argument: SEXP) -> R_xlen_t {
    let value = checked(owner.sexp(length_argument));
    let t = value.typeof_();
    if !matches!(t, SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP) {
        crate::mainutils::essentials::base_error("invalid arguments");
    }
    if value.len() == 1 {
        let dn = if t == SEXPTYPE::REALSXP {
            checked(value.try_real_elt(0))
        } else {
            let element = if t == SEXPTYPE::LGLSXP {
                value.logical_elt(0)
            } else {
                value.integer_elt(0)
            };
            match element {
                Some(v) if v != NA_INTEGER => v as c_double,
                _ => f64::NAN,
            }
        };
        if dn.is_nan() || dn < 0.0 {
            crate::mainutils::essentials::base_error("invalid arguments");
        }
        dn as R_xlen_t
    } else {
        value.len()
    }
}

// ---------------------------------------------------------------------------
// Helper: isNumeric check
// ---------------------------------------------------------------------------

fn isNumeric(x: SEXP) -> bool {
    if x.is_null() {
        return false;
    }
    let t = TYPEOF(x);
    t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP
}

// ---------------------------------------------------------------------------
// Helper: asReal
// ---------------------------------------------------------------------------

fn as_real(owner: crate::sexp::owner::OwnerToken<'_>, x: SEXP) -> c_double {
    if x.is_null() {
        return NA_REAL;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::REALSXP {
        return checked(owner.sexp(x)).real_elt(0).unwrap_or(NA_REAL);
    }
    if t == SEXPTYPE::INTSXP {
        let v = checked(owner.sexp(x)).integer_elt(0).unwrap_or(NA_INTEGER);
        if v == NA_INTEGER {
            return NA_REAL;
        }
        return v as c_double;
    }
    if t == SEXPTYPE::LGLSXP {
        let v = checked(owner.sexp(x)).logical_elt(0).unwrap_or(NA_INTEGER);
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

fn as_integer(owner: crate::sexp::owner::OwnerToken<'_>, x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::INTSXP {
        return checked(owner.sexp(x)).integer_elt(0).unwrap_or(NA_INTEGER);
    }
    if t == SEXPTYPE::REALSXP {
        let v = checked(owner.sexp(x)).real_elt(0).unwrap_or(NA_REAL);
        if v.is_nan() || v < c_int::MIN as c_double || v > c_int::MAX as c_double {
            return NA_INTEGER;
        }
        return v as c_int;
    }
    if t == SEXPTYPE::LGLSXP {
        return checked(owner.sexp(x)).logical_elt(0).unwrap_or(NA_INTEGER);
    }
    NA_INTEGER
}

// ---------------------------------------------------------------------------
// External declarations
// ---------------------------------------------------------------------------

fn allocMatrix(
    owner: crate::sexp::owner::OwnerToken<'_>,
    sexptype: c_int,
    nrow: c_int,
    ncol: c_int,
) -> SEXP {
    let ans = Rf_allocVector(sexptype, nrow * ncol);
    let _ans_guard = protect(ans);
    let dim = Rf_allocVector(SEXPTYPE::INTSXP, 2);
    let _dim_guard = protect(dim);
    let mut dim_out = checked(SexpMut::try_from_checked(checked(owner.sexp(dim))));
    checked(dim_out.try_set_integer_elt(0, nrow));
    checked(dim_out.try_set_integer_elt(1, ncol));
    crate::attrib_core::setAttrib(ans, crate::attrib_core::R_DimSymbol(), dim);
    ans
}

use crate::library::stats::rcont::rcont2_slices;

// ---------------------------------------------------------------------------
// random1 -- 1-parameter random sampling
// ---------------------------------------------------------------------------

fn random_n<const K: usize>(
    sn: SEXP,
    sargs: [SEXP; K],
    sample: impl Fn([c_double; K]) -> c_double,
    type_: SEXPTYPE,
) -> SEXP {
    // SAFETY: the raw ABI caller retains the active runtime across dispatch.
    let owner = checked(unsafe { crate::sexp::owner::OwnerToken::current() });
    for argument in sargs {
        if !isNumeric(argument) {
            crate::mainutils::essentials::base_error("invalid arguments");
        }
    }
    let n = result_length(owner, sn);
    if n == 0 {
        // SAFETY: the owner is active; a zero-length result skips coercion
        // and sampling entirely, matching the legacy dispatch order.
        return {
            let x = Rf_allocVector3(type_, 0);
            let _x_guard = protect(x);
            x
        };
    }
    if sargs
        .iter()
        .any(|argument| checked(owner.sexp(*argument)).len() < 1)
    {
        // The owner is active; the fresh vector is protected while its
        // payload is filled for the checked extent n.
        let x = Rf_allocVector3(type_, n);
        let _x_guard = protect(x);
        let mut output = checked(SexpMut::try_from_checked(checked(owner.sexp(x))));
        for i in 0..n {
            if type_ == SEXPTYPE::INTSXP {
                checked(output.try_set_integer_elt(i, NA_INTEGER));
            } else {
                checked(output.try_set_real_elt(i, NA_REAL));
            }
        }
        // SAFETY: the warning is dispatched under the active runtime.
        {
            crate::mainutils::errors::Rf_warning1(c"NAs produced".as_ptr())
        };
        return x;
    }

    // SAFETY: the owner is active. The legacy allocator/coercion keep their
    // typed resource error and GC retry semantics; every result is protected
    // inside the same block before it can be collected.
    let (coerced, _arg_guards) = {
        let coerced: [SEXP; K] = sargs.map(|s| coerceVector(s, SEXPTYPE::REALSXP.as_c_int()));
        let guards = coerced.map(|s| protect(s));
        (coerced, guards)
    };
    let sources = coerced.map(|c| checked(owner.sexp(c)));
    let lens: [R_xlen_t; K] = std::array::from_fn(|i| sources[i].len());

    // SAFETY: RNG state is process-global behind the interpreter lock; the
    // loan covers the whole sampling region including any overflow result
    // allocation, so the state is released exactly once per dispatch.
    {
        GetRNGstate();
        let mut naflag = false;
        if type_ == SEXPTYPE::INTSXP {
            let x = Rf_allocVector3(type_, n);
            let _x_guard = protect(x);
            let mut output = checked(SexpMut::try_from_checked(checked(owner.sexp(x))));
            let mut cursors = [0i64; K];
            let mut i0: R_xlen_t = 0;
            let mut overflow_rx = 0.0f64;
            let mut overflow = false;
            for i in 0..n {
                let draw = sample(std::array::from_fn(|j| {
                    checked(sources[j].try_real_elt(cursors[j]))
                }));
                for (cursor, length) in cursors.iter_mut().zip(lens) {
                    *cursor += 1;
                    if *cursor == length {
                        *cursor = 0;
                    }
                }
                if ISNAN(draw) {
                    naflag = true;
                    checked(output.try_set_integer_elt(i, NA_INTEGER));
                } else if draw > c_int::MAX as c_double || draw <= c_int::MIN as c_double {
                    i0 = i;
                    overflow_rx = draw;
                    overflow = true;
                    break;
                } else {
                    checked(output.try_set_integer_elt(i, draw as c_int));
                }
            }
            if overflow {
                let xr = Rf_allocVector3(SEXPTYPE::REALSXP, n);
                let _xr_guard = protect(xr);
                let mut real_out = checked(SexpMut::try_from_checked(checked(owner.sexp(xr))));
                for i in 0..i0 {
                    let value = output.integer_elt(i).unwrap_or_else(|| {
                        crate::mainutils::essentials::base_error("integer result read failed")
                    });
                    checked(real_out.try_set_real_elt(i, value as c_double));
                }
                checked(real_out.try_set_real_elt(i0, overflow_rx));
                for i in (i0 + 1)..n {
                    let draw = sample(std::array::from_fn(|j| {
                        checked(sources[j].try_real_elt(cursors[j]))
                    }));
                    for (cursor, length) in cursors.iter_mut().zip(lens) {
                        *cursor += 1;
                        if *cursor == length {
                            *cursor = 0;
                        }
                    }
                    if ISNAN(draw) {
                        naflag = true;
                    }
                    checked(real_out.try_set_real_elt(i, draw));
                }
                PutRNGstate();
                if naflag {
                    crate::mainutils::errors::Rf_warning1(c"NAs produced".as_ptr());
                }
                return xr;
            }
            PutRNGstate();
            if naflag {
                crate::mainutils::errors::Rf_warning1(c"NAs produced".as_ptr());
            }
            return x;
        }
        let x = Rf_allocVector3(type_, n);
        let _x_guard = protect(x);
        let mut output = checked(SexpMut::try_from_checked(checked(owner.sexp(x))));
        let mut cursors = [0i64; K];
        for i in 0..n {
            let draw = sample(std::array::from_fn(|j| {
                checked(sources[j].try_real_elt(cursors[j]))
            }));
            for (cursor, length) in cursors.iter_mut().zip(lens) {
                *cursor += 1;
                if *cursor == length {
                    *cursor = 0;
                }
            }
            if ISNAN(draw) {
                naflag = true;
            }
            checked(output.try_set_real_elt(i, draw));
        }
        PutRNGstate();
        if naflag {
            crate::mainutils::errors::Rf_warning1(c"NAs produced".as_ptr());
        }
        x
    }
}

// ---------------------------------------------------------------------------
// 1-parameter random samplers
// ---------------------------------------------------------------------------

pub fn do_rchisq(sn: SEXP, sa: SEXP) -> SEXP {
    random_n(
        sn,
        [sa],
        |v| crate::nmath::dist::chisq::rchisq_inner(v[0]),
        SEXPTYPE::REALSXP,
    )
}

pub fn do_rexp(sn: SEXP, sa: SEXP) -> SEXP {
    random_n(
        sn,
        [sa],
        |v| crate::nmath::dist::exponential::rexp_inner(v[0]),
        SEXPTYPE::REALSXP,
    )
}

pub fn do_rgeom(sn: SEXP, sa: SEXP) -> SEXP {
    random_n(
        sn,
        [sa],
        |v| crate::nmath::dist::geometric::rgeom_inner(v[0]),
        SEXPTYPE::INTSXP,
    )
}

pub fn do_rpois(sn: SEXP, sa: SEXP) -> SEXP {
    random_n(
        sn,
        [sa],
        |v| crate::nmath::dist::poisson::rpois_inner(v[0]),
        SEXPTYPE::INTSXP,
    )
}

pub fn do_rt(sn: SEXP, sa: SEXP) -> SEXP {
    random_n(
        sn,
        [sa],
        |v| crate::nmath::dist::t_dist::rt_inner(v[0]),
        SEXPTYPE::REALSXP,
    )
}

pub fn do_rsignrank(sn: SEXP, sa: SEXP) -> SEXP {
    random_n(
        sn,
        [sa],
        |v| crate::nmath::dist::signrank::rsignrank_inner(v[0]),
        SEXPTYPE::INTSXP,
    )
}

// ---------------------------------------------------------------------------
// 2-parameter random samplers
// ---------------------------------------------------------------------------

pub fn do_rbeta(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb],
        |v| crate::nmath::dist::beta::rbeta_inner(v[0], v[1]),
        SEXPTYPE::REALSXP,
    )
}

pub fn do_rbinom(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb],
        |v| crate::nmath::dist::binomial::rbinom_inner(v[0], v[1]),
        SEXPTYPE::INTSXP,
    )
}

pub fn do_rcauchy(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb],
        |v| crate::nmath::dist::cauchy::rcauchy_inner(v[0], v[1]),
        SEXPTYPE::REALSXP,
    )
}

pub fn do_rf(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb],
        |v| crate::nmath::dist::f_dist::rf_inner(v[0], v[1]),
        SEXPTYPE::REALSXP,
    )
}

pub fn do_rgamma(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb],
        |v| crate::nmath::dist::gamma::rgamma_inner(v[0], v[1]),
        SEXPTYPE::REALSXP,
    )
}

pub fn do_rlnorm(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb],
        |v| crate::nmath::dist::lnorm::rlnorm_inner(v[0], v[1]),
        SEXPTYPE::REALSXP,
    )
}

pub fn do_rlogis(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb],
        |v| crate::nmath::dist::logistic::rlogis_inner(v[0], v[1]),
        SEXPTYPE::REALSXP,
    )
}

pub fn do_rnbinom(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb],
        |v| crate::nmath::dist::nbinom::rnbinom_inner(v[0], v[1]),
        SEXPTYPE::INTSXP,
    )
}

pub fn do_rnorm(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb],
        |v| crate::nmath::dist::normal::rnorm_inner(v[0], v[1]),
        SEXPTYPE::REALSXP,
    )
}

pub fn do_runif(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb],
        |v| crate::nmath::dist::uniform::runif_inner(v[0], v[1]),
        SEXPTYPE::REALSXP,
    )
}

pub fn do_rweibull(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb],
        |v| crate::nmath::dist::weibull::rweibull_inner(v[0], v[1]),
        SEXPTYPE::REALSXP,
    )
}

pub fn do_rwilcox(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb],
        |v| crate::nmath::dist::wilcox::rwilcox_inner(v[0], v[1]),
        SEXPTYPE::INTSXP,
    )
}

pub fn do_rnchisq(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb],
        |v| crate::nmath::dist::nchisq::rnchisq_inner(v[0], v[1]),
        SEXPTYPE::REALSXP,
    )
}

pub fn do_rnbinom_mu(sn: SEXP, sa: SEXP, sb: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb],
        |v| crate::nmath::dist::nbinom::rnbinom_mu_inner(v[0], v[1]),
        SEXPTYPE::INTSXP,
    )
}

// ---------------------------------------------------------------------------
// 3-parameter random samplers
// ---------------------------------------------------------------------------

pub fn do_rhyper(sn: SEXP, sa: SEXP, sb: SEXP, sc: SEXP) -> SEXP {
    random_n(
        sn,
        [sa, sb, sc],
        |v| crate::nmath::dist::hypergeometric::rhyper_inner(v[0], v[1], v[2]),
        SEXPTYPE::INTSXP,
    )
}

// ---------------------------------------------------------------------------
// FixupProb -- normalize probability vector
// ---------------------------------------------------------------------------

fn fixup_prob(p: &mut [c_double]) {
    let mut sum = 0.0;
    let mut npos = 0;
    for value in p.iter() {
        if !R_FINITE(*value) {
            {
                Rf_error(b"NA in probability vector\0".as_ptr() as *const _);
            }
            return;
        }
        if *value < 0.0 {
            {
                Rf_error(b"negative probability\0".as_ptr() as *const _);
            }
            return;
        }
        if *value > 0.0 {
            npos += 1;
            sum += *value;
        }
    }
    if npos == 0 {
        {
            Rf_error(b"no positive probabilities\0".as_ptr() as *const _);
        }
        return;
    }
    for value in p.iter_mut() {
        *value /= sum;
    }
}

// ---------------------------------------------------------------------------
// do_rmultinom -- multinomial random sampling
// ---------------------------------------------------------------------------

pub fn do_rmultinom(sn: SEXP, ssize: SEXP, prob: SEXP) -> SEXP {
    // SAFETY: the raw ABI caller retains the active runtime across dispatch.
    let owner = checked(unsafe { crate::sexp::owner::OwnerToken::current() });
    let n = as_integer(owner, sn);
    let size = as_integer(owner, ssize);
    if n == NA_INTEGER || n < 0 {
        Rf_error(b"invalid first argument 'n'\0".as_ptr() as *const _);
        return R_NilValue();
    }
    if size == NA_INTEGER || size < 0 {
        Rf_error(b"invalid second argument 'size'\0".as_ptr() as *const _);
        return R_NilValue();
    }
    let prob = coerceVector(prob, SEXPTYPE::REALSXP.as_c_int());
    let k = LENGTH(prob);
    let _prob_guard = protect(prob);
    let prob_sexp = checked(owner.sexp(prob));
    let mut prob_buf = vec![0.0; k as usize];
    for (i, value) in prob_buf.iter_mut().enumerate() {
        *value = checked(prob_sexp.try_real_elt(i as R_xlen_t));
    }
    fixup_prob(&mut prob_buf);

    GetRNGstate();
    let ans = allocMatrix(owner, SEXPTYPE::INTSXP.into(), k, n);
    let _ans_guard = protect(ans);
    let mut cells = checked(SexpMut::try_from_checked(checked(owner.sexp(ans))));
    let mut rn_buf: Vec<f64> = vec![0.0; k as usize];
    for i in 0..n as usize {
        crate::nmath::dist::multinom::rmultinom_inner(size, &prob_buf, &mut rn_buf);
        // Copy f64 results to integer output
        for j in 0..k as usize {
            checked(
                cells.try_set_integer_elt((i * k as usize + j) as R_xlen_t, rn_buf[j] as c_int),
            );
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

// ---------------------------------------------------------------------------
// r2dtable -- random 2-way tables with given marginals
// ---------------------------------------------------------------------------

pub fn r2dtable(n: SEXP, r: SEXP, c: SEXP) -> SEXP {
    // SAFETY: the raw ABI caller retains the active runtime across dispatch.
    let owner = checked(unsafe { crate::sexp::owner::OwnerToken::current() });
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

    let n_of_samples = checked(owner.sexp(n)).integer_elt(0).unwrap_or(NA_INTEGER);
    let r_sexp = checked(owner.sexp(r));
    let c_sexp = checked(owner.sexp(c));
    let row_sums: Vec<c_int> = r_sexp.clone().iter_integer().collect();
    let col_sums: Vec<c_int> = c_sexp.clone().iter_integer().collect();

    // Compute total cases as sum of row sums
    let mut n_of_cases: c_int = 0;
    for value in &row_sums {
        n_of_cases += *value;
    }

    // Log-factorials
    let mut fact = vec![0.0f64; (n_of_cases + 1) as usize];
    for i in 1..=n_of_cases as usize {
        fact[i] = crate::nmath::special::gamma::lgammafn((i + 1) as c_double);
    }

    let mut jwork = vec![0; nc as usize];
    let ans = Rf_allocVector(SEXPTYPE::VECSXP, n_of_samples);
    let _ans_guard = protect(ans);

    GetRNGstate();

    for i in 0..n_of_samples {
        let tmp = allocMatrix(owner, SEXPTYPE::INTSXP.into(), nr, nc);
        let _tmp_guard = protect(tmp);
        // Buffers satisfy the dimension contract; degenerate margins are
        // rejected by the checked wrapper instead of producing garbage.
        let mut table = vec![0; (nr * nc) as usize];
        let _ = rcont2_slices(
            nr, nc, &row_sums, &col_sums, n_of_cases, &fact, &mut jwork, &mut table,
        );
        let mut tmp_out = checked(SexpMut::try_from_checked(checked(owner.sexp(tmp))));
        for (index, value) in table.iter().enumerate() {
            checked(tmp_out.try_set_integer_elt(index as R_xlen_t, *value));
        }
        SET_VECTOR_ELT(ans, i as R_xlen_t, tmp);
    }

    PutRNGstate();
    ans
}

// ---------------------------------------------------------------------------
// R-level adapters -- the stock stats closures bake in argument defaults and
// ncp/prob-vs-mu/rate-vs-scale dispatch before calling the .Call entry points.
// The port has no R closures, so these adapters reproduce that front end.
// ---------------------------------------------------------------------------

fn adapter_absent(x: SEXP) -> bool {
    x.is_null() || x == R_NilValue() || x == R_MissingArg()
}

fn missing_required(call: SEXP, name: &str) -> ! {
    crate::main::errors::errorcall_str(
        call,
        &format!("argument \"{name}\" is missing, with no default"),
    )
}

/// Bind `args` to `names` through GNU `matchArgs` (exact, partial, positional).
fn match_formals(call: SEXP, args: SEXP, names: &[&str]) -> Vec<SEXP> {
    // SAFETY: the raw ABI caller retains the active runtime; the matcher only
    // inspects the argument list and returns borrowed slots.
    unsafe { crate::mainutils::match_mod::match_formal_slots(call, args, names) }
}

fn require_slot(call: SEXP, slot: SEXP, name: &str) -> SEXP {
    if adapter_absent(slot) {
        missing_required(call, name);
    }
    slot
}

/// `x` or a ScalarReal(default) when the argument is absent; freshly
/// allocated defaults are protected via `guards` for the adapter's scope.
fn with_default(
    x: SEXP,
    default: c_double,
    guards: &mut Vec<crate::sexp::protect::ProtectGuard>,
) -> SEXP {
    if adapter_absent(x) {
        let s = Rf_ScalarReal(default);
        guards.push(protect(s));
        s
    } else {
        x
    }
}

/// Vectorized `1/x` (the `rexp` closure's `1/rate`), with R's Inf/NA rules.
fn reciprocal_vector(owner: crate::sexp::owner::OwnerToken<'_>, x: SEXP) -> SEXP {
    let v = coerceVector(x, SEXPTYPE::REALSXP.as_c_int());
    let _v_guard = protect(v);
    let src = checked(owner.sexp(v));
    let n = src.len();
    let out = Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int);
    let _out_guard = protect(out);
    let mut dst = checked(SexpMut::try_from_checked(checked(owner.sexp(out))));
    for i in 0..n {
        checked(dst.try_set_real_elt(i, 1.0 / checked(src.try_real_elt(i))));
    }
    out
}

/// Elementwise `s * x` for a numeric vector (e.g. rbeta's `2 * shape1`).
fn scaled_vector(owner: crate::sexp::owner::OwnerToken<'_>, x: SEXP, s: c_double) -> SEXP {
    let v = coerceVector(x, SEXPTYPE::REALSXP.as_c_int());
    let _v_guard = protect(v);
    let src = checked(owner.sexp(v));
    let n = src.len();
    let out = Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int);
    let _out_guard = protect(out);
    let mut dst = checked(SexpMut::try_from_checked(checked(owner.sexp(out))));
    for i in 0..n {
        checked(dst.try_set_real_elt(i, s * checked(src.try_real_elt(i))));
    }
    out
}

/// Elementwise binary op on two REALSXP results with stock recycling of the
/// shorter operand (used by the ncp composition closures).
fn vector_binop(
    owner: crate::sexp::owner::OwnerToken<'_>,
    a: SEXP,
    b: SEXP,
    f: fn(c_double, c_double) -> c_double,
) -> SEXP {
    let lhs = checked(owner.sexp(a));
    let rhs = checked(owner.sexp(b));
    let na = lhs.len() as usize;
    let nb = rhs.len() as usize;
    let n = na.max(nb);
    let out = Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int);
    let _out_guard = protect(out);
    let mut dst = checked(SexpMut::try_from_checked(checked(owner.sexp(out))));
    for i in 0..n {
        let value = f(
            checked(lhs.try_real_elt((i % na) as R_xlen_t)),
            checked(rhs.try_real_elt((i % nb) as R_xlen_t)),
        );
        checked(dst.try_set_real_elt(i as R_xlen_t, value));
    }
    out
}

fn div(a: c_double, b: c_double) -> c_double {
    a / b
}

fn add(a: c_double, b: c_double) -> c_double {
    a + b
}

/// Elementwise sqrt on a REALSXP vector.
fn sqrt_vector(owner: crate::sexp::owner::OwnerToken<'_>, x: SEXP) -> SEXP {
    let src = checked(owner.sexp(x));
    let n = src.len();
    let out = Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int);
    let _out_guard = protect(out);
    let mut dst = checked(SexpMut::try_from_checked(checked(owner.sexp(out))));
    for i in 0..n {
        checked(dst.try_set_real_elt(i, checked(src.try_real_elt(i)).sqrt()));
    }
    out
}

pub fn do_rchisq_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["n", "df", "ncp"]);
    let n = require_slot(call, m[0], "n");
    let df = require_slot(call, m[1], "df");
    if adapter_absent(m[2]) {
        do_rchisq(n, df)
    } else {
        do_rnchisq(n, df, m[2])
    }
}

pub fn do_rexp_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // SAFETY: builtin dispatch holds the runtime lock across this call.
    let owner = checked(unsafe { crate::sexp::owner::OwnerToken::current() });
    let m = match_formals(call, args, &["n", "rate"]);
    let n = require_slot(call, m[0], "n");
    let mut guards = Vec::new();
    let scale = reciprocal_vector(owner, with_default(m[1], 1.0, &mut guards));
    do_rexp(n, scale)
}

pub fn do_rgeom_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["n", "prob"]);
    do_rgeom(
        require_slot(call, m[0], "n"),
        require_slot(call, m[1], "prob"),
    )
}

pub fn do_rpois_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["n", "lambda"]);
    do_rpois(
        require_slot(call, m[0], "n"),
        require_slot(call, m[1], "lambda"),
    )
}

pub fn do_rt_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // SAFETY: builtin dispatch holds the runtime lock across this call.
    let owner = checked(unsafe { crate::sexp::owner::OwnerToken::current() });
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
        let ratio = vector_binop(owner, chi, df, div);
        let _ratio_guard = protect(ratio);
        let root = sqrt_vector(owner, ratio);
        let _root_guard = protect(root);
        vector_binop(owner, z, root, div)
    }
}

pub fn do_rsignrank_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["nn", "n"]);
    do_rsignrank(
        require_slot(call, m[0], "nn"),
        require_slot(call, m[1], "n"),
    )
}

pub fn do_rbeta_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // SAFETY: builtin dispatch holds the runtime lock across this call.
    let owner = checked(unsafe { crate::sexp::owner::OwnerToken::current() });
    let m = match_formals(call, args, &["n", "shape1", "shape2", "ncp"]);
    let n = require_slot(call, m[0], "n");
    let shape1 = require_slot(call, m[1], "shape1");
    let shape2 = require_slot(call, m[2], "shape2");
    let ncp = m[3];
    if adapter_absent(ncp) {
        do_rbeta(n, shape1, shape2)
    } else {
        // X <- rchisq(n, 2*shape1, ncp); X/(X + rchisq(n, 2*shape2))
        let df1 = scaled_vector(owner, shape1, 2.0);
        let _df1_guard = protect(df1);
        let x = do_rnchisq(n, df1, ncp);
        let _x_guard = protect(x);
        let df2 = scaled_vector(owner, shape2, 2.0);
        let _df2_guard = protect(df2);
        let y = do_rchisq(n, df2);
        let _y_guard = protect(y);
        let sum = vector_binop(owner, x, y, add);
        let _sum_guard = protect(sum);
        vector_binop(owner, x, sum, div)
    }
}

pub fn do_rbinom_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["n", "size", "prob"]);
    do_rbinom(
        require_slot(call, m[0], "n"),
        require_slot(call, m[1], "size"),
        require_slot(call, m[2], "prob"),
    )
}

pub fn do_rmultinom_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["n", "size", "prob"]);
    do_rmultinom(
        require_slot(call, m[0], "n"),
        require_slot(call, m[1], "size"),
        require_slot(call, m[2], "prob"),
    )
}

pub fn do_r2dtable_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

pub fn do_rcauchy_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["n", "location", "scale"]);
    let n = require_slot(call, m[0], "n");
    let mut guards = Vec::new();
    do_rcauchy(
        n,
        with_default(m[1], 0.0, &mut guards),
        with_default(m[2], 1.0, &mut guards),
    )
}

pub fn do_rf_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // SAFETY: builtin dispatch holds the runtime lock across this call.
    let owner = checked(unsafe { crate::sexp::owner::OwnerToken::current() });
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
        let num = vector_binop(owner, num0, df1, div);
        let _num_guard = protect(num);
        let den0 = do_rchisq(n, df2);
        let _den0_guard = protect(den0);
        let den = vector_binop(owner, den0, df2, div);
        let _den_guard = protect(den);
        vector_binop(owner, num, den, div)
    }
}

pub fn do_rgamma_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // SAFETY: builtin dispatch holds the runtime lock across this call.
    let owner = checked(unsafe { crate::sexp::owner::OwnerToken::current() });
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
        let r0 = checked(owner.sexp(rv)).real_elt(0).unwrap_or(NA_REAL);
        let s0 = checked(owner.sexp(sv)).real_elt(0).unwrap_or(NA_REAL);
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
        reciprocal_vector(owner, with_default(rate, 1.0, &mut guards))
    };
    do_rgamma(n, shape, scale_arg)
}

pub fn do_rlnorm_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["n", "meanlog", "sdlog"]);
    let n = require_slot(call, m[0], "n");
    let mut guards = Vec::new();
    do_rlnorm(
        n,
        with_default(m[1], 0.0, &mut guards),
        with_default(m[2], 1.0, &mut guards),
    )
}

pub fn do_rlogis_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["n", "location", "scale"]);
    let n = require_slot(call, m[0], "n");
    let mut guards = Vec::new();
    do_rlogis(
        n,
        with_default(m[1], 0.0, &mut guards),
        with_default(m[2], 1.0, &mut guards),
    )
}

pub fn do_rnbinom_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

pub fn do_rnorm_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["n", "mean", "sd"]);
    let n = require_slot(call, m[0], "n");
    let mut guards = Vec::new();
    do_rnorm(
        n,
        with_default(m[1], 0.0, &mut guards),
        with_default(m[2], 1.0, &mut guards),
    )
}

pub fn do_runif_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["n", "min", "max"]);
    let n = require_slot(call, m[0], "n");
    let mut guards = Vec::new();
    do_runif(
        n,
        with_default(m[1], 0.0, &mut guards),
        with_default(m[2], 1.0, &mut guards),
    )
}

pub fn do_rweibull_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["n", "shape", "scale"]);
    let n = require_slot(call, m[0], "n");
    let shape = require_slot(call, m[1], "shape");
    let mut guards = Vec::new();
    do_rweibull(n, shape, with_default(m[2], 1.0, &mut guards))
}

pub fn do_rwilcox_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["nn", "m", "n"]);
    do_rwilcox(
        require_slot(call, m[0], "nn"),
        require_slot(call, m[1], "m"),
        require_slot(call, m[2], "n"),
    )
}

pub fn do_rhyper_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["nn", "m", "n", "k"]);
    do_rhyper(
        require_slot(call, m[0], "nn"),
        require_slot(call, m[1], "m"),
        require_slot(call, m[2], "n"),
        require_slot(call, m[3], "k"),
    )
}
