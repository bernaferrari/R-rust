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

pub fn do_dchisq(sa: SEXP, sb: SEXP, sI: SEXP) -> SEXP {
    math2_1(sa, sb, sI, chisq::dchisq_inner)
}

// --- dexp(x, scale, log) ---

pub fn do_dexp(sa: SEXP, sb: SEXP, sI: SEXP) -> SEXP {
    math2_1(sa, sb, sI, exponential::dexp_inner)
}

// --- dgeom(x, p, log) ---

pub fn do_dgeom(sa: SEXP, sb: SEXP, sI: SEXP) -> SEXP {
    math2_1(sa, sb, sI, geometric::dgeom_inner)
}

// --- dpois(x, lambda, log) ---

pub fn do_dpois(sa: SEXP, sb: SEXP, sI: SEXP) -> SEXP {
    math2_1(sa, sb, sI, poisson::dpois_inner)
}

// --- dt(x, df, log) ---

pub fn do_dt(sa: SEXP, sb: SEXP, sI: SEXP) -> SEXP {
    math2_1(sa, sb, sI, t_dist::dt_inner)
}

// --- dsignrank(x, n, log) ---

pub fn do_dsignrank(sa: SEXP, sb: SEXP, sI: SEXP) -> SEXP {
    signrank::evaluate(sa, sb, sI, R_NilValue(), signrank::Kind::Density)
}

// --- pchisq(x, df, lower_tail, log_p) ---

pub fn do_pchisq(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, chisq::pchisq_inner)
}

// --- qchisq(p, df, lower_tail, log_p) ---

pub fn do_qchisq(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, chisq::qchisq_inner)
}

// --- pexp(x, scale, lower_tail, log_p) ---

pub fn do_pexp(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, exponential::pexp_inner)
}

// --- qexp(p, scale, lower_tail, log_p) ---

pub fn do_qexp(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, exponential::qexp_inner)
}

// --- pgeom(x, p, lower_tail, log_p) ---

pub fn do_pgeom(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, geometric::pgeom_inner)
}

// --- qgeom(p, prob, lower_tail, log_p) ---

pub fn do_qgeom(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, geometric::qgeom_inner)
}

// --- ppois(x, lambda, lower_tail, log_p) ---

pub fn do_ppois(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, poisson::ppois_inner)
}

// --- qpois(p, lambda, lower_tail, log_p) ---

pub fn do_qpois(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, poisson::qpois_inner)
}

// --- pt(x, df, lower_tail, log_p) ---

pub fn do_pt(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, t_dist::pt_inner)
}

// --- qt(p, df, lower_tail, log_p) ---

pub fn do_qt(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math2_2(sa, sb, sI, sJ, t_dist::qt_inner)
}

// --- psignrank(x, n, lower_tail, log_p) ---

pub fn do_psignrank(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    signrank::evaluate(sa, sb, sI, sJ, signrank::Kind::Cdf)
}

// --- qsignrank(p, n, lower_tail, log_p) ---

pub fn do_qsignrank(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    signrank::evaluate(sa, sb, sI, sJ, signrank::Kind::Quantile)
}

// ===========================================================================
// Math3 functions (3-argument)
// ===========================================================================

// --- dbeta(x, a, b, log) ---

pub fn do_dbeta(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, beta::dbeta_inner)
}

// --- dbinom(x, n, p, log) ---

pub fn do_dbinom(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, binomial::dbinom_inner)
}

// --- dcauchy(x, location, scale, log) ---

pub fn do_dcauchy(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, cauchy::dcauchy_inner)
}

// --- df(x, df1, df2, log) ---

pub fn do_df(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, f_dist::df_inner)
}

// --- dgamma(x, shape, scale, log) ---

pub fn do_dgamma(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, gamma::dgamma_inner)
}

// --- dlnorm(x, meanlog, sdlog, log) ---

pub fn do_dlnorm(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, lnorm::dlnorm_inner)
}

// --- dlogis(x, location, scale, log) ---

pub fn do_dlogis(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, logistic::dlogis_inner)
}

// --- dnbinom(x, size, prob, log) ---

pub fn do_dnbinom(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, nbinom::dnbinom_inner)
}

// --- dnbinom_mu(x, size, mu, log) ---

pub fn do_dnbinom_mu(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, nbinom::dnbinom_mu_inner)
}

// --- dnorm(x, mean, sd, log) ---

pub fn do_dnorm(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, normal::dnorm4_inner)
}

// --- dweibull(x, shape, scale, log) ---

pub fn do_dweibull(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, weibull::dweibull_inner)
}

// --- dunif(x, min, max, log) ---

pub fn do_dunif(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, uniform::dunif_inner)
}

// --- dnt(x, df, ncp, log) ---

pub fn do_dnt(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, nt_dist::dnt_inner)
}

// --- dnchisq(x, df, ncp, log) ---

pub fn do_dnchisq(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, nchisq::dnchisq_inner)
}

// --- dwilcox(x, m, n, log) ---

pub fn do_dwilcox(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP) -> SEXP {
    math3_1(sa, sb, sc, sI, wilcox::dwilcox_inner)
}

// ===========================================================================
// Math3_2 functions (3-argument with 2 int flags)
// ===========================================================================

// --- pbeta, qbeta ---

pub fn do_pbeta(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, beta::pbeta_inner)
}

pub fn do_qbeta(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, beta::qbeta_inner)
}

// --- pbinom, qbinom ---

pub fn do_pbinom(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, binomial::pbinom_inner)
}

pub fn do_qbinom(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, binomial::qbinom_inner)
}

// --- pcauchy, qcauchy ---

pub fn do_pcauchy(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, cauchy::pcauchy_inner)
}

pub fn do_qcauchy(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, cauchy::qcauchy_inner)
}

// --- pf, qf ---

pub fn do_pf(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, f_dist::pf_inner)
}

pub fn do_qf(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, f_dist::qf_inner)
}

// --- pgamma, qgamma ---

pub fn do_pgamma(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, gamma::pgamma_inner)
}

pub fn do_qgamma(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, gamma::qgamma_inner)
}

// --- plnorm, qlnorm ---

pub fn do_plnorm(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, lnorm::plnorm_inner)
}

pub fn do_qlnorm(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, lnorm::qlnorm_inner)
}

// --- plogis, qlogis ---

pub fn do_plogis(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, logistic::plogis_inner)
}

pub fn do_qlogis(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, logistic::qlogis_inner)
}

// --- pnbinom, qnbinom ---

pub fn do_pnbinom(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, nbinom::pnbinom_inner)
}

pub fn do_qnbinom(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, nbinom::qnbinom_inner)
}

// --- pnbinom_mu, qnbinom_mu ---

pub fn do_pnbinom_mu(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, nbinom::pnbinom_mu_inner)
}

pub fn do_qnbinom_mu(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, nbinom::qnbinom_mu_inner)
}

// --- pnorm, qnorm ---

pub fn do_pnorm(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, normal::pnorm5_inner)
}

pub fn do_qnorm(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, normal::qnorm5_inner)
}

// --- pweibull, qweibull ---

pub fn do_pweibull(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, weibull::pweibull_inner)
}

pub fn do_qweibull(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, weibull::qweibull_inner)
}

// --- punif, qunif ---

pub fn do_punif(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, uniform::punif_inner)
}

pub fn do_qunif(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, uniform::qunif_inner)
}

// --- pnt, qnt ---

pub fn do_pnt(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, nt_dist::pnt_inner)
}

pub fn do_qnt(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, nt_dist::qnt_inner)
}

// --- pnchisq, qnchisq ---

pub fn do_pnchisq(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, nchisq::pnchisq_inner)
}

pub fn do_qnchisq(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, nchisq::qnchisq_inner)
}

// --- pwilcox, qwilcox ---

pub fn do_pwilcox(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wilcox::pwilcox_inner)
}

pub fn do_qwilcox(sa: SEXP, sb: SEXP, sc: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math3_2(sa, sb, sc, sI, sJ, wilcox::qwilcox_inner)
}

// ===========================================================================
// Math4 functions (4-argument)
// ===========================================================================

// --- dhyper(x, r, b, n, log) ---

pub fn do_dhyper(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP) -> SEXP {
    math4_1(sa, sb, sc, sd, sI, hypergeometric::dhyper_inner)
}

// --- dnbeta(x, a, b, ncp, log) ---

pub fn do_dnbeta(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP) -> SEXP {
    math4_1(sa, sb, sc, sd, sI, nbeta::dnbeta_inner)
}

// --- dnf(x, df1, df2, ncp, log) ---

pub fn do_dnf(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP) -> SEXP {
    math4_1(sa, sb, sc, sd, sI, nf_dist::dnf_inner)
}

// --- phyper, qhyper ---

pub fn do_phyper(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, hypergeometric::phyper_inner)
}

pub fn do_qhyper(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, hypergeometric::qhyper_inner)
}

// --- pnbeta, qnbeta ---

pub fn do_pnbeta(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, nbeta::pnbeta_inner)
}

pub fn do_qnbeta(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, nbeta::qnbeta_inner)
}

// --- pnf, qnf ---

pub fn do_pnf(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, nf_dist::pnf_inner)
}

pub fn do_qnf(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, nf_dist::qnf_inner)
}

// --- ptukey, qtukey ---

pub fn do_ptukey(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, tukey::ptukey_inner)
}

pub fn do_qtukey(sa: SEXP, sb: SEXP, sc: SEXP, sd: SEXP, sI: SEXP, sJ: SEXP) -> SEXP {
    math4_2(sa, sb, sc, sd, sI, sJ, tukey::qtukey_inner)
}

// ===========================================================================
// signrank_free / wilcox_free -- stubs (no-op, no caching in our port)
// ===========================================================================

pub fn stats_signrank_free(_args: SEXP) -> SEXP {
    R_NilValue()
}

pub fn stats_wilcox_free(_args: SEXP) -> SEXP {
    R_NilValue()
}

pub(crate) unsafe extern "C-unwind" fn c_signrank_free(_args: SEXP) -> SEXP {
    stats_signrank_free(R_NilValue())
}
pub(crate) unsafe extern "C-unwind" fn c_wilcox_free(_args: SEXP) -> SEXP {
    stats_wilcox_free(R_NilValue())
}

macro_rules! wrap3 {
    ($c:ident, $do:ident) => {
        unsafe extern "C-unwind" fn $c(a: SEXP, b: SEXP, c: SEXP) -> SEXP {
            $do(a, b, c)
        }
    };
}
macro_rules! wrap4 {
    ($c:ident, $do:ident) => {
        unsafe extern "C-unwind" fn $c(a: SEXP, b: SEXP, c: SEXP, d: SEXP) -> SEXP {
            $do(a, b, c, d)
        }
    };
}
macro_rules! wrap5 {
    ($c:ident, $do:ident) => {
        unsafe extern "C-unwind" fn $c(a: SEXP, b: SEXP, c: SEXP, d: SEXP, e: SEXP) -> SEXP {
            $do(a, b, c, d, e)
        }
    };
}
macro_rules! wrap6 {
    ($c:ident, $do:ident) => {
        unsafe extern "C-unwind" fn $c(
            a: SEXP,
            b: SEXP,
            c: SEXP,
            d: SEXP,
            e: SEXP,
            f: SEXP,
        ) -> SEXP {
            $do(a, b, c, d, e, f)
        }
    };
}

wrap3!(c_dchisq, do_dchisq);
wrap3!(c_dexp, do_dexp);
wrap3!(c_dgeom, do_dgeom);
wrap3!(c_dpois, do_dpois);
wrap3!(c_dt, do_dt);
wrap3!(c_dsignrank, do_dsignrank);

wrap4!(c_pchisq, do_pchisq);
wrap4!(c_qchisq, do_qchisq);
wrap4!(c_pexp, do_pexp);
wrap4!(c_qexp, do_qexp);
wrap4!(c_pgeom, do_pgeom);
wrap4!(c_qgeom, do_qgeom);
wrap4!(c_ppois, do_ppois);
wrap4!(c_qpois, do_qpois);
wrap4!(c_pt, do_pt);
wrap4!(c_qt, do_qt);
wrap4!(c_psignrank, do_psignrank);
wrap4!(c_qsignrank, do_qsignrank);
wrap4!(c_dbeta, do_dbeta);
wrap4!(c_dbinom, do_dbinom);
wrap4!(c_dcauchy, do_dcauchy);
wrap4!(c_df, do_df);
wrap4!(c_dgamma, do_dgamma);
wrap4!(c_dlnorm, do_dlnorm);
wrap4!(c_dlogis, do_dlogis);
wrap4!(c_dnbinom, do_dnbinom);
wrap4!(c_dnbinom_mu, do_dnbinom_mu);
wrap4!(c_dnorm, do_dnorm);
wrap4!(c_dweibull, do_dweibull);
wrap4!(c_dunif, do_dunif);
wrap4!(c_dnt, do_dnt);
wrap4!(c_dnchisq, do_dnchisq);
wrap4!(c_dwilcox, do_dwilcox);

wrap5!(c_pbeta, do_pbeta);
wrap5!(c_qbeta, do_qbeta);
wrap5!(c_pbinom, do_pbinom);
wrap5!(c_qbinom, do_qbinom);
wrap5!(c_pcauchy, do_pcauchy);
wrap5!(c_qcauchy, do_qcauchy);
wrap5!(c_pf, do_pf);
wrap5!(c_qf, do_qf);
wrap5!(c_pgamma, do_pgamma);
wrap5!(c_qgamma, do_qgamma);
wrap5!(c_plnorm, do_plnorm);
wrap5!(c_qlnorm, do_qlnorm);
wrap5!(c_plogis, do_plogis);
wrap5!(c_qlogis, do_qlogis);
wrap5!(c_pnbinom, do_pnbinom);
wrap5!(c_qnbinom, do_qnbinom);
wrap5!(c_pnbinom_mu, do_pnbinom_mu);
wrap5!(c_qnbinom_mu, do_qnbinom_mu);
wrap5!(c_pnorm, do_pnorm);
wrap5!(c_qnorm, do_qnorm);
wrap5!(c_pweibull, do_pweibull);
wrap5!(c_qweibull, do_qweibull);
wrap5!(c_punif, do_punif);
wrap5!(c_qunif, do_qunif);
wrap5!(c_pnt, do_pnt);
wrap5!(c_qnt, do_qnt);
wrap5!(c_pnchisq, do_pnchisq);
wrap5!(c_qnchisq, do_qnchisq);
wrap5!(c_pwilcox, do_pwilcox);
wrap5!(c_qwilcox, do_qwilcox);
wrap5!(c_dhyper, do_dhyper);
wrap5!(c_dnbeta, do_dnbeta);
wrap5!(c_dnf, do_dnf);

wrap6!(c_phyper, do_phyper);
wrap6!(c_qhyper, do_qhyper);
wrap6!(c_pnbeta, do_pnbeta);
wrap6!(c_qnbeta, do_qnbeta);
wrap6!(c_pnf, do_pnf);
wrap6!(c_qnf, do_qnf);
wrap6!(c_ptukey, do_ptukey);
wrap6!(c_qtukey, do_qtukey);


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
