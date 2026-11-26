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

use crate::main::errors::{Rf_error, Rf_error_cstr};
use crate::nmath::dist::{
    beta, binomial, cauchy, chisq, exponential, f_dist, gamma, geometric, hypergeometric, lnorm,
    logistic, nbeta, nbinom, nchisq, nf_dist, normal, nt_dist, poisson, t_dist, tukey, uniform,
    weibull, wilcox,
};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// Helper: isNumeric
// ---------------------------------------------------------------------------

fn isNumeric(owner: crate::sexp::owner::OwnerToken<'_>, x: SEXP) -> bool {
    owner
        .require_active()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    if x.is_null() {
        return false;
    }
    let t = TYPEOF(x);
    // GNU isNumeric accepts logicals but excludes integer factors. Without
    // this guard attribute copying could attach factor class to REAL output.
    // SAFETY: the owner is active, so the class lookup runs under the live
    // runtime even though the checked operand also carries the factor test.
    let numeric = t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::LGLSXP
        || (t == SEXPTYPE::INTSXP && crate::mainutils::objects::inherits2(x, c"factor") == 0);
    owner
        .require_active()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    numeric
}

// ---------------------------------------------------------------------------
// Helper: asInteger
// ---------------------------------------------------------------------------

fn as_integer(owner: crate::sexp::owner::OwnerToken<'_>, value: SEXP) -> c_int {
    owner
        .require_active()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    // SAFETY: the owner is active; integer coercion may evaluate providers
    // and condition handlers under the live runtime.
    let value = crate::main::coerce::asInteger(value);
    owner
        .require_active()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    value
}

fn prepare_inputs(owner: crate::sexp::owner::OwnerToken<'_>, inputs: &[(SEXP, R_xlen_t)]) {
    for &(raw, _) in inputs {
        owner
            .require_active()
            .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
        let value = owner
            .sexp(raw)
            .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
        crate::sexp::altrep::force_materialization(&value)
            .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    }
}
fn check_inputs(owner: crate::sexp::owner::OwnerToken<'_>, inputs: &[(SEXP, R_xlen_t)]) {
    owner
        .require_active()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    for &(raw, length) in inputs {
        let value = owner
            .sexp(raw)
            .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
        if value.len() != length {
            crate::mainutils::essentials::base_error(
                "distribution input length changed during flag coercion",
            );
        }
    }
}

// Stock SHALLOW_DUPLICATE_ATTRIB, with checked roots retaining the exact source
// attribute graph across allocation/collection and original-owner rechecks.
fn copy_math_attributes(owner: crate::sexp::owner::OwnerToken<'_>, to: SEXP, from: SEXP) {
    owner
        .require_active()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    let target = owner
        .sexp(to)
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    let source = owner
        .sexp(from)
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    let attributes = source
        .try_attrib()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    // The owner is active; the checked attribute root keeps the source graph
    // live across duplication and protection.
    let copied = crate::mainutils::seq::shallow_duplicate(attributes.as_raw());
    let _copied = protect(copied);
    owner
        .require_active()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    // SAFETY: both checked objects remain rooted while their headers are
    // rewritten to mirror GNU's attribute copy.

    SET_ATTRIB(target.as_raw(), copied);
    SET_OBJECT(target.as_raw(), OBJECT(source.as_raw()));
    if crate::mainutils::coerce::IS_S4_OBJECT(source.as_raw()) != 0 {
        SET_S4_OBJECT(target.as_raw());
    } else {
        UNSET_S4_OBJECT(target.as_raw());
    }

    owner
        .require_active()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
}
fn finish_math_attributes(
    owner: crate::sexp::owner::OwnerToken<'_>,
    result: SEXP,
    inputs: &[(SEXP, R_xlen_t)],
    length: R_xlen_t,
) {
    for &(source, n) in inputs {
        if n == length {
            copy_math_attributes(owner, result, source);
            break;
        }
    }
}

/// GNU `FINISH_MathN`: `warning(_("NaNs produced"))` when a finite input
/// produced a NaN. eprintln is not a warning condition (`assertWarning`).
fn nans_produced() {
    // SAFETY: the caller dispatches the warning under an active owner.

    crate::mainutils::errors::Rf_warning1(c"NaNs produced");
}

// ---------------------------------------------------------------------------
// Checked numerical recycling for registered d/p/q routines
// ---------------------------------------------------------------------------

fn checked<T>(result: crate::sexp::object::SexpResult<T>) -> T {
    result.unwrap_or_else(|error| crate::mainutils::essentials::base_error(error.to_string()))
}

fn math_recycle<const N: usize, const F: usize>(
    owner: crate::sexp::owner::OwnerToken<'_>,
    arguments: [SEXP; N],
    flags: [SEXP; F],
    compute: impl Fn([f64; N], [bool; F]) -> f64,
) -> SEXP {
    use crate::sexp::object::SexpMut;

    checked(owner.require_active());
    let source = arguments.map(|raw| checked(owner.sexp(raw)));
    for operand in &source {
        // The checked operand roots its entire graph. The existing
        // predicate also checks the original owner across class lookup.
        if !isNumeric(owner, operand.as_raw()) {
            // SAFETY: the owner is active; error dispatch may longjmp.
            Rf_error_cstr(c"Non-numeric argument to mathematical function");
            unreachable!("Rf_error does not return");
        }
    }

    let lengths = std::array::from_fn::<_, N, _>(|i| source[i].len());
    let empty = lengths.contains(&0);
    let n = if empty {
        0
    } else {
        lengths.iter().copied().max().unwrap_or_default()
    };
    let allocate_result = || {
        checked(owner.require_active());
        // SAFETY: the owner is active. Keep the legacy allocator's typed
        // resource error and GC retry semantics before wrapping the result.
        let raw = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        checked(owner.require_active());
        checked(owner.sexp(raw))
    };
    if empty {
        let result = allocate_result();
        if lengths[0] == 0 {
            copy_math_attributes(owner, result.as_raw(), source[0].as_raw());
        }
        return result.as_raw();
    }

    let inputs = source.map(|operand| {
        checked(owner.require_active());
        // SAFETY: the checked operand roots its graph across coercion.
        let raw = crate::main::coerce::coerceVector(operand.as_raw(), SEXPTYPE::REALSXP.as_c_int());
        checked(owner.require_active());
        checked(owner.sexp(raw))
    });
    let mut result = checked(SexpMut::try_from_checked(allocate_result()));

    for input in &inputs {
        checked(crate::sexp::altrep::force_materialization(input));
        checked(owner.require_active());
    }
    let flags = flags.map(|raw| {
        let value = checked(owner.sexp(raw));
        // The flag remains rooted while integer coercion can run
        // providers or condition handlers. as_integer checks the original owner.
        as_integer(owner, value.as_raw()) != 0
    });
    checked(owner.require_active());
    for (input, length) in inputs.iter().zip(lengths) {
        if input.len() != length {
            crate::mainutils::essentials::base_error(
                "distribution input length changed during flag coercion",
            );
        }
    }

    let mut cursors = [0; N];
    let mut produced_nan = false;
    for i in 0..n {
        let values = std::array::from_fn(|j| checked(inputs[j].try_real_elt(cursors[j])));
        let value = if values.into_iter().any(R_IsNA) {
            NA_REAL
        } else if values.into_iter().any(f64::is_nan) {
            f64::NAN
        } else {
            let value = compute(values, flags);
            produced_nan |= value.is_nan();
            value
        };
        checked(result.try_set_real_elt(i, value));
        for (position, length) in cursors.iter_mut().zip(lengths) {
            *position += 1;
            if *position == length {
                *position = 0;
            }
        }
    }

    let result = result.freeze();
    if produced_nan {
        nans_produced();
    }
    if let Some(position) = lengths.iter().position(|&length| length == n) {
        copy_math_attributes(owner, result.as_raw(), inputs[position].as_raw());
    }
    result.as_raw()
}

fn math2_1(a: SEXP, b: SEXP, i: SEXP, compute: fn(c_double, c_double, bool) -> f64) -> SEXP {
    // SAFETY: the raw ABI caller retains the active runtime across dispatch.
    let owner = crate::library::stats::checked::current_owner();
    math_recycle(owner, [a, b], [i], |[a, b], [i]| compute(a, b, i))
}

fn math2_2(
    a: SEXP,
    b: SEXP,
    i: SEXP,
    j: SEXP,
    compute: fn(c_double, c_double, bool, bool) -> f64,
) -> SEXP {
    // SAFETY: the raw ABI caller retains the active runtime across dispatch.
    let owner = crate::library::stats::checked::current_owner();
    math_recycle(owner, [a, b], [i, j], |[a, b], [i, j]| compute(a, b, i, j))
}

fn math3_1(
    a: SEXP,
    b: SEXP,
    c: SEXP,
    i: SEXP,
    compute: fn(c_double, c_double, c_double, bool) -> f64,
) -> SEXP {
    // SAFETY: the raw ABI caller retains the active runtime across dispatch.
    let owner = crate::library::stats::checked::current_owner();
    math_recycle(owner, [a, b, c], [i], |[a, b, c], [i]| compute(a, b, c, i))
}

fn math3_2(
    a: SEXP,
    b: SEXP,
    c: SEXP,
    i: SEXP,
    j: SEXP,
    compute: fn(c_double, c_double, c_double, bool, bool) -> f64,
) -> SEXP {
    // SAFETY: the raw ABI caller retains the active runtime across dispatch.
    let owner = crate::library::stats::checked::current_owner();
    math_recycle(owner, [a, b, c], [i, j], |[a, b, c], [i, j]| {
        compute(a, b, c, i, j)
    })
}

fn math4_1(
    a: SEXP,
    b: SEXP,
    c: SEXP,
    d: SEXP,
    i: SEXP,
    compute: fn(c_double, c_double, c_double, c_double, bool) -> f64,
) -> SEXP {
    // SAFETY: the raw ABI caller retains the active runtime across dispatch.
    let owner = crate::library::stats::checked::current_owner();
    math_recycle(owner, [a, b, c, d], [i], |[a, b, c, d], [i]| {
        compute(a, b, c, d, i)
    })
}

fn math4_2(
    a: SEXP,
    b: SEXP,
    c: SEXP,
    d: SEXP,
    i: SEXP,
    j: SEXP,
    compute: fn(c_double, c_double, c_double, c_double, bool, bool) -> f64,
) -> SEXP {
    // SAFETY: the raw ABI caller retains the active runtime across dispatch.
    let owner = crate::library::stats::checked::current_owner();
    math_recycle(owner, [a, b, c, d], [i, j], |[a, b, c, d], [i, j]| {
        compute(a, b, c, d, i, j)
    })
}

// ===========================================================================
// R's registered distribution entry points. MathN coerces flags once and
// passes booleans directly to the safe scalar kernels.
// ===========================================================================

// --- dchisq(x, df, log) ---


// --- dexp(x, scale, log) ---


// --- dgeom(x, p, log) ---


// --- dpois(x, lambda, log) ---


// --- dt(x, df, log) ---


// --- dsignrank(x, n, log) ---


// --- pchisq(x, df, lower_tail, log_p) ---


// --- qchisq(p, df, lower_tail, log_p) ---


// --- pexp(x, scale, lower_tail, log_p) ---


// --- qexp(p, scale, lower_tail, log_p) ---


// --- pgeom(x, p, lower_tail, log_p) ---


// --- qgeom(p, prob, lower_tail, log_p) ---


// --- ppois(x, lambda, lower_tail, log_p) ---


// --- qpois(p, lambda, lower_tail, log_p) ---


// --- pt(x, df, lower_tail, log_p) ---


// --- qt(p, df, lower_tail, log_p) ---


// --- psignrank(x, n, lower_tail, log_p) ---


// --- qsignrank(p, n, lower_tail, log_p) ---


// ===========================================================================
// Math3 functions (3-argument)
// ===========================================================================

// --- dbeta(x, a, b, log) ---


// --- dbinom(x, n, p, log) ---


// --- dcauchy(x, location, scale, log) ---


// --- df(x, df1, df2, log) ---


// --- dgamma(x, shape, scale, log) ---


// --- dlnorm(x, meanlog, sdlog, log) ---


// --- dlogis(x, location, scale, log) ---


// --- dnbinom(x, size, prob, log) ---


// --- dnbinom_mu(x, size, mu, log) ---


// --- dnorm(x, mean, sd, log) ---


// --- dweibull(x, shape, scale, log) ---


// --- dunif(x, min, max, log) ---


// --- dnt(x, df, ncp, log) ---


// --- dnchisq(x, df, ncp, log) ---


// --- dwilcox(x, m, n, log) ---


// ===========================================================================
// Math3_2 functions (3-argument with 2 int flags)
// ===========================================================================

// --- pbeta, qbeta ---



// --- pbinom, qbinom ---



// --- pcauchy, qcauchy ---



// --- pf, qf ---



// --- pgamma, qgamma ---



// --- plnorm, qlnorm ---



// --- plogis, qlogis ---



// --- pnbinom, qnbinom ---



// --- pnbinom_mu, qnbinom_mu ---



// --- pnorm, qnorm ---



// --- pweibull, qweibull ---



// --- punif, qunif ---



// --- pnt, qnt ---



// --- pnchisq, qnchisq ---



// --- pwilcox, qwilcox ---



// ===========================================================================
// Math4 functions (4-argument)
// ===========================================================================

// --- dhyper(x, r, b, n, log) ---


// --- dnbeta(x, a, b, ncp, log) ---


// --- dnf(x, df1, df2, ncp, log) ---


// --- phyper, qhyper ---



// --- pnbeta, qnbeta ---



// --- pnf, qnf ---



// --- ptukey, qtukey ---



// ===========================================================================
// signrank_free / wilcox_free -- stubs (no-op, no caching in our port)
// ===========================================================================

pub fn stats_signrank_free(_args: SEXP) -> SEXP {
    R_NilValue()
}

pub fn stats_wilcox_free(_args: SEXP) -> SEXP {
    R_NilValue()
}

pub(crate) unsafe fn c_signrank_free(_args: SEXP) -> SEXP {
    stats_signrank_free(R_NilValue())
}
pub(crate) unsafe fn c_wilcox_free(_args: SEXP) -> SEXP {
    stats_wilcox_free(R_NilValue())
}


pub(crate) unsafe fn c_psignrank(a: SEXP, b: SEXP, i: SEXP, j: SEXP) -> SEXP {
    signrank::evaluate(a, b, i, j, signrank::Kind::Cdf)
}
pub(crate) unsafe fn c_qsignrank(a: SEXP, b: SEXP, i: SEXP, j: SEXP) -> SEXP {
    signrank::evaluate(a, b, i, j, signrank::Kind::Quantile)
}
pub(crate) unsafe fn c_dsignrank(a: SEXP, b: SEXP, i: SEXP) -> SEXP {
    signrank::evaluate(a, b, i, R_NilValue(), signrank::Kind::Density)
}

macro_rules! wrap3 {
    ($c:ident, $helper:ident, $kernel:expr) => {
        unsafe fn $c(a: SEXP, b: SEXP, c: SEXP) -> SEXP {
            $helper(a, b, c, $kernel)
        }
    };
}
macro_rules! wrap4 {
    ($c:ident, $helper:ident, $kernel:expr) => {
        unsafe fn $c(a: SEXP, b: SEXP, c: SEXP, d: SEXP) -> SEXP {
            $helper(a, b, c, d, $kernel)
        }
    };
}
macro_rules! wrap5 {
    ($c:ident, $helper:ident, $kernel:expr) => {
        unsafe fn $c(a: SEXP, b: SEXP, c: SEXP, d: SEXP, e: SEXP) -> SEXP {
            $helper(a, b, c, d, e, $kernel)
        }
    };
}
macro_rules! wrap6 {
    ($c:ident, $helper:ident, $kernel:expr) => {
        unsafe fn $c(
            a: SEXP,
            b: SEXP,
            c: SEXP,
            d: SEXP,
            e: SEXP,
            f: SEXP,
        ) -> SEXP {
            $helper(a, b, c, d, e, f, $kernel)
        }
    };
}

wrap3!(c_dchisq, math2_1, chisq::dchisq_inner);
wrap3!(c_dexp, math2_1, exponential::dexp_inner);
wrap3!(c_dgeom, math2_1, geometric::dgeom_inner);
wrap3!(c_dpois, math2_1, poisson::dpois_inner);
wrap3!(c_dt, math2_1, t_dist::dt_inner);

wrap4!(c_pchisq, math2_2, chisq::pchisq_inner);
wrap4!(c_qchisq, math2_2, chisq::qchisq_inner);
wrap4!(c_pexp, math2_2, exponential::pexp_inner);
wrap4!(c_qexp, math2_2, exponential::qexp_inner);
wrap4!(c_pgeom, math2_2, geometric::pgeom_inner);
wrap4!(c_qgeom, math2_2, geometric::qgeom_inner);
wrap4!(c_ppois, math2_2, poisson::ppois_inner);
wrap4!(c_qpois, math2_2, poisson::qpois_inner);
wrap4!(c_pt, math2_2, t_dist::pt_inner);
wrap4!(c_qt, math2_2, t_dist::qt_inner);
wrap4!(c_dbeta, math3_1, beta::dbeta_inner);
wrap4!(c_dbinom, math3_1, binomial::dbinom_inner);
wrap4!(c_dcauchy, math3_1, cauchy::dcauchy_inner);
wrap4!(c_df, math3_1, f_dist::df_inner);
wrap4!(c_dgamma, math3_1, gamma::dgamma_inner);
wrap4!(c_dlnorm, math3_1, lnorm::dlnorm_inner);
wrap4!(c_dlogis, math3_1, logistic::dlogis_inner);
wrap4!(c_dnbinom, math3_1, nbinom::dnbinom_inner);
wrap4!(c_dnbinom_mu, math3_1, nbinom::dnbinom_mu_inner);
wrap4!(c_dnorm, math3_1, normal::dnorm4_inner);
wrap4!(c_dweibull, math3_1, weibull::dweibull_inner);
wrap4!(c_dunif, math3_1, uniform::dunif_inner);
wrap4!(c_dnt, math3_1, nt_dist::dnt_inner);
wrap4!(c_dnchisq, math3_1, nchisq::dnchisq_inner);
wrap4!(c_dwilcox, math3_1, wilcox::dwilcox_inner);

wrap5!(c_pbeta, math3_2, beta::pbeta_inner);
wrap5!(c_qbeta, math3_2, beta::qbeta_inner);
wrap5!(c_pbinom, math3_2, binomial::pbinom_inner);
wrap5!(c_qbinom, math3_2, binomial::qbinom_inner);
wrap5!(c_pcauchy, math3_2, cauchy::pcauchy_inner);
wrap5!(c_qcauchy, math3_2, cauchy::qcauchy_inner);
wrap5!(c_pf, math3_2, f_dist::pf_inner);
wrap5!(c_qf, math3_2, f_dist::qf_inner);
wrap5!(c_pgamma, math3_2, gamma::pgamma_inner);
wrap5!(c_qgamma, math3_2, gamma::qgamma_inner);
wrap5!(c_plnorm, math3_2, lnorm::plnorm_inner);
wrap5!(c_qlnorm, math3_2, lnorm::qlnorm_inner);
wrap5!(c_plogis, math3_2, logistic::plogis_inner);
wrap5!(c_qlogis, math3_2, logistic::qlogis_inner);
wrap5!(c_pnbinom, math3_2, nbinom::pnbinom_inner);
wrap5!(c_qnbinom, math3_2, nbinom::qnbinom_inner);
wrap5!(c_pnbinom_mu, math3_2, nbinom::pnbinom_mu_inner);
wrap5!(c_qnbinom_mu, math3_2, nbinom::qnbinom_mu_inner);
wrap5!(c_pnorm, math3_2, normal::pnorm5_inner);
wrap5!(c_qnorm, math3_2, normal::qnorm5_inner);
wrap5!(c_pweibull, math3_2, weibull::pweibull_inner);
wrap5!(c_qweibull, math3_2, weibull::qweibull_inner);
wrap5!(c_punif, math3_2, uniform::punif_inner);
wrap5!(c_qunif, math3_2, uniform::qunif_inner);
wrap5!(c_pnt, math3_2, nt_dist::pnt_inner);
wrap5!(c_qnt, math3_2, nt_dist::qnt_inner);
wrap5!(c_pnchisq, math3_2, nchisq::pnchisq_inner);
wrap5!(c_qnchisq, math3_2, nchisq::qnchisq_inner);
wrap5!(c_pwilcox, math3_2, wilcox::pwilcox_inner);
wrap5!(c_qwilcox, math3_2, wilcox::qwilcox_inner);
wrap5!(c_dhyper, math4_1, hypergeometric::dhyper_inner);
wrap5!(c_dnbeta, math4_1, nbeta::dnbeta_inner);
wrap5!(c_dnf, math4_1, nf_dist::dnf_inner);

wrap6!(c_phyper, math4_2, hypergeometric::phyper_inner);
wrap6!(c_qhyper, math4_2, hypergeometric::qhyper_inner);
wrap6!(c_pnbeta, math4_2, nbeta::pnbeta_inner);
wrap6!(c_qnbeta, math4_2, nbeta::qnbeta_inner);
wrap6!(c_pnf, math4_2, nf_dist::pnf_inner);
wrap6!(c_qnf, math4_2, nf_dist::qnf_inner);
wrap6!(c_ptukey, math4_2, tukey::ptukey_inner);
wrap6!(c_qtukey, math4_2, tukey::qtukey_inner);


pub(crate) fn lookup_call(name: &str) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "dchisq" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_dchisq),
        )),
        "dexp" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_dexp),
        )),
        "dgeom" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_dgeom),
        )),
        "dpois" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_dpois),
        )),
        "dt" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_dt),
        )),
        "dsignrank" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_dsignrank),
        )),
        "pchisq" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_pchisq),
        )),
        "qchisq" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_qchisq),
        )),
        "pexp" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_pexp),
        )),
        "qexp" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_qexp),
        )),
        "pgeom" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_pgeom),
        )),
        "qgeom" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_qgeom),
        )),
        "ppois" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_ppois),
        )),
        "qpois" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_qpois),
        )),
        "pt" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_pt),
        )),
        "qt" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_qt),
        )),
        "psignrank" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_psignrank),
        )),
        "qsignrank" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_qsignrank),
        )),
        "dbeta" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_dbeta),
        )),
        "dbinom" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_dbinom),
        )),
        "dcauchy" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_dcauchy),
        )),
        "df" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_df),
        )),
        "dgamma" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_dgamma),
        )),
        "dlnorm" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_dlnorm),
        )),
        "dlogis" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_dlogis),
        )),
        "dnbinom" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_dnbinom),
        )),
        "dnbinom_mu" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_dnbinom_mu),
        )),
        "dnorm" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_dnorm),
        )),
        "dweibull" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_dweibull),
        )),
        "dunif" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_dunif),
        )),
        "dnt" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_dnt),
        )),
        "dnchisq" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_dnchisq),
        )),
        "dwilcox" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_dwilcox),
        )),
        "pbeta" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_pbeta),
        )),
        "qbeta" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qbeta),
        )),
        "pbinom" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_pbinom),
        )),
        "qbinom" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qbinom),
        )),
        "pcauchy" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_pcauchy),
        )),
        "qcauchy" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qcauchy),
        )),
        "pf" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_pf),
        )),
        "qf" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qf),
        )),
        "pgamma" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_pgamma),
        )),
        "qgamma" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qgamma),
        )),
        "plnorm" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_plnorm),
        )),
        "qlnorm" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qlnorm),
        )),
        "plogis" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_plogis),
        )),
        "qlogis" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qlogis),
        )),
        "pnbinom" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_pnbinom),
        )),
        "qnbinom" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qnbinom),
        )),
        "pnbinom_mu" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_pnbinom_mu),
        )),
        "qnbinom_mu" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qnbinom_mu),
        )),
        "pnorm" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_pnorm),
        )),
        "qnorm" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qnorm),
        )),
        "pweibull" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_pweibull),
        )),
        "qweibull" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qweibull),
        )),
        "punif" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_punif),
        )),
        "qunif" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qunif),
        )),
        "pnt" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_pnt),
        )),
        "qnt" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qnt),
        )),
        "pnchisq" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_pnchisq),
        )),
        "qnchisq" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qnchisq),
        )),
        "pwilcox" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_pwilcox),
        )),
        "qwilcox" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_qwilcox),
        )),
        "dhyper" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_dhyper),
        )),
        "dnbeta" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_dnbeta),
        )),
        "dnf" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_dnf),
        )),
        "phyper" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args6(c_phyper),
        )),
        "qhyper" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args6(c_qhyper),
        )),
        "pnbeta" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args6(c_pnbeta),
        )),
        "qnbeta" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args6(c_qnbeta),
        )),
        "pnf" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args6(c_pnf),
        )),
        "qnf" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args6(c_qnf),
        )),
        "ptukey" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args6(c_ptukey),
        )),
        "qtukey" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args6(c_qtukey),
        )),
        "signrank_free" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_signrank_free,
            crate::mainutils::native_routines::PayloadArity::Fixed(0),
        )),
        "wilcox_free" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_wilcox_free,
            crate::mainutils::native_routines::PayloadArity::Fixed(0),
        )),
        _ => None,
    }
}

pub const DISTN_CALL_NAMES: &[&str] = &[
    "C_dchisq",
    "C_dexp",
    "C_dgeom",
    "C_dpois",
    "C_dt",
    "C_dsignrank",
    "C_pchisq",
    "C_qchisq",
    "C_pexp",
    "C_qexp",
    "C_pgeom",
    "C_qgeom",
    "C_ppois",
    "C_qpois",
    "C_pt",
    "C_qt",
    "C_psignrank",
    "C_qsignrank",
    "C_dbeta",
    "C_dbinom",
    "C_dcauchy",
    "C_df",
    "C_dgamma",
    "C_dlnorm",
    "C_dlogis",
    "C_dnbinom",
    "C_dnbinom_mu",
    "C_dnorm",
    "C_dweibull",
    "C_dunif",
    "C_dnt",
    "C_dnchisq",
    "C_dwilcox",
    "C_pbeta",
    "C_qbeta",
    "C_pbinom",
    "C_qbinom",
    "C_pcauchy",
    "C_qcauchy",
    "C_pf",
    "C_qf",
    "C_pgamma",
    "C_qgamma",
    "C_plnorm",
    "C_qlnorm",
    "C_plogis",
    "C_qlogis",
    "C_pnbinom",
    "C_qnbinom",
    "C_pnbinom_mu",
    "C_qnbinom_mu",
    "C_pnorm",
    "C_qnorm",
    "C_pweibull",
    "C_qweibull",
    "C_punif",
    "C_qunif",
    "C_pnt",
    "C_qnt",
    "C_pnchisq",
    "C_qnchisq",
    "C_pwilcox",
    "C_qwilcox",
    "C_dhyper",
    "C_dnbeta",
    "C_dnf",
    "C_phyper",
    "C_qhyper",
    "C_pnbeta",
    "C_qnbeta",
    "C_pnf",
    "C_qnf",
    "C_ptukey",
    "C_qtukey",
    "C_signrank_free",
    "C_wilcox_free",
];

#[cfg(test)]
mod flag_tests;

#[cfg(test)]
mod attribute_tests;

pub(crate) mod signrank;

#[cfg(test)]
mod signrank_admission_tests;
