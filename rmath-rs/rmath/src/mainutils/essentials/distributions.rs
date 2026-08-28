// Distribution-function builtins: d*/p*/q* wrappers (dnorm..dmultinom) and
// their shared vectorization helpers (do_dist_unary family, dist_match,
// map_real_distribution). Extracted from essentials.rs as the first step of
// incremental decomposition (rport-btb7). Re-exported by essentials.rs so the
// builtin registration table paths (crate::mainutils::essentials::do_dnorm etc.)
// are unchanged. Shared helpers (real_or_default, elt_real_safe, logical_arg)
// remain in essentials.rs and are reached via super::.
#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
use super::*;

// ---------------------------------------------------------------------------
// Argument matching
// ---------------------------------------------------------------------------

/// Match supplied args against a formals-name table the way stock R's
/// `matchArgs_NR` does for closures without `...`:
/// 1. exact tag matches,
/// 2. partial tag matches (erroring on ambiguity),
/// 3. untagged args fill remaining slots positionally.
///
/// Slots never supplied stay `R_NilValue`; a supplied arg that matches no
/// formal is an "unused argument" error, mirroring stock R (e.g. `pnorm(1,
/// foo=2)` errors).
unsafe fn dist_match(args: SEXP, names: &[&str]) -> Vec<SEXP> {
    unsafe {
        let mut out: Vec<SEXP> = names.iter().map(|_| R_NilValue()).collect();
        let mut filled = vec![false; names.len()];
        let mut supplied: Vec<(Option<String>, SEXP)> = Vec::new();
        let mut cur = args;
        while !cur.is_null() && cur != R_NilValue() {
            supplied.push((tag_name(cur), CAR(cur)));
            cur = CDR(cur);
        }

        // Pass 1: exact tag matches.
        let mut matched = vec![false; supplied.len()];
        for i in 0..supplied.len() {
            if let Some(tag) = supplied[i].0.as_deref() {
                if let Some(j) = names.iter().position(|n| *n == tag) {
                    out[j] = supplied[i].1;
                    filled[j] = true;
                    matched[i] = true;
                }
            }
        }

        // Pass 2: partial tag matches against unfilled slots.
        for i in 0..supplied.len() {
            if matched[i] {
                continue;
            }
            let tag = supplied[i].0.as_deref().unwrap_or("");
            if tag.is_empty() {
                continue;
            }
            let candidates: Vec<usize> = (0..names.len())
                .filter(|&j| !filled[j] && names[j].starts_with(tag))
                .collect();
            match candidates.len() {
                1 => {
                    out[candidates[0]] = supplied[i].1;
                    filled[candidates[0]] = true;
                    matched[i] = true;
                }
                0 => {}
                _ => base_error(format!(
                    "argument {} matches multiple formal arguments",
                    i + 1
                )),
            }
        }

        // Pass 3: positional fill of remaining slots.
        let mut slot = 0;
        for i in 0..supplied.len() {
            if matched[i] {
                continue;
            }
            if supplied[i].0.is_some() {
                let tag = supplied[i].0.clone().unwrap_or_default();
                base_error(format!("unused argument ({tag} = ...)"));
            }
            while slot < names.len() && filled[slot] {
                slot += 1;
            }
            if slot >= names.len() {
                base_error("unused argument");
            }
            out[slot] = supplied[i].1;
            filled[slot] = true;
            matched[i] = true;
        }

        out
    }
}

/// Resolve gamma-family `rate`/`scale` (mutually exclusive formals, stock
/// default rate = 1, i.e. scale = 1).
unsafe fn dist_scale(rate_arg: SEXP, scale_arg: SEXP) -> f64 {
    unsafe {
        if !scale_arg.is_null() && scale_arg != R_NilValue() {
            real_or_default(scale_arg, 1.0)
        } else if !rate_arg.is_null() && rate_arg != R_NilValue() {
            1.0 / real_or_default(rate_arg, 1.0)
        } else {
            1.0
        }
    }
}

// ---------------------------------------------------------------------------
// Distribution functions: dnorm, pnorm, qnorm, dpois, ppois
// ---------------------------------------------------------------------------

/// R's `dnorm(x, mean=0, sd=1)` — normal density.
pub unsafe fn do_dnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(
        args,
        &["x", "mean", "sd", "log"],
        0.0,
        1.0,
        crate::dist::normal::dnorm4_inner,
    )
}

/// R's `pnorm(q, mean=0, sd=1)` — normal CDF.
pub unsafe fn do_pnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "mean", "sd", "lower.tail", "log.p"],
        0.0,
        1.0,
        crate::dist::normal::pnorm5_inner,
    )
}

/// R's `qnorm(p, mean=0, sd=1)` — normal quantile.
pub unsafe fn do_qnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "mean", "sd", "lower.tail", "log.p"],
        0.0,
        1.0,
        crate::dist::normal::qnorm5_inner,
    )
}

/// R's `dpois(x, lambda, log=FALSE)` — Poisson density.
pub unsafe fn do_dpois(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(
        args,
        &["x", "lambda", "log"],
        1.0,
        0.0,
        |x, lam, _, give_log| crate::dist::poisson::dpois_inner(x, lam, give_log),
    )
}

/// R's `ppois(q, lambda, lower.tail=TRUE, log.p=FALSE)` — Poisson CDF.
pub unsafe fn do_ppois(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "lambda", "lower.tail", "log.p"],
        1.0,
        0.0,
        |q, lam, _, lower_tail, log_p| crate::dist::poisson::ppois_inner(q, lam, lower_tail, log_p),
    )
}

/// R's `qpois(p, lambda, lower.tail=TRUE, log.p=FALSE)` — Poisson quantile.
pub unsafe fn do_qpois(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "lambda", "lower.tail", "log.p"],
        1.0,
        0.0,
        |p, lam, _, lower_tail, log_p| crate::dist::poisson::qpois_inner(p, lam, lower_tail, log_p),
    )
}

/// R's `dbinom(x, size, prob, log=FALSE)` — binomial density.
pub unsafe fn do_dbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(
        args,
        &["x", "size", "prob", "log"],
        1.0,
        0.5,
        crate::dist::binomial::dbinom_inner,
    )
}

/// R's `pbinom(q, size, prob, lower.tail=TRUE, log.p=FALSE)` — binomial CDF.
pub unsafe fn do_pbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "size", "prob", "lower.tail", "log.p"],
        1.0,
        0.5,
        |q, n, p, lower_tail, log_p| {
            crate::dist::binomial::pbinom_inner(q, n, p, lower_tail, log_p)
        },
    )
}

/// R's `qbinom(p, size, prob, lower.tail=TRUE, log.p=FALSE)` — binomial quantile.
pub unsafe fn do_qbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "size", "prob", "lower.tail", "log.p"],
        1.0,
        0.5,
        |p, n, pr, lower_tail, log_p| {
            crate::dist::binomial::qbinom_inner(p, n, pr, lower_tail, log_p)
        },
    )
}

/// R's `dexp(x, rate=1, log=FALSE)` — exponential density.
pub unsafe fn do_dexp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(
        args,
        &["x", "rate", "log"],
        1.0,
        0.0,
        |x, rate, _, give_log| crate::dist::exponential::dexp_inner(x, 1.0 / rate, give_log),
    )
}

/// R's `pexp(q, rate=1, lower.tail=TRUE, log.p=FALSE)` — exponential CDF.
pub unsafe fn do_pexp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "rate", "lower.tail", "log.p"],
        1.0,
        0.0,
        |q, rate, _, lower_tail, log_p| {
            crate::dist::exponential::pexp_inner(q, 1.0 / rate, lower_tail, log_p)
        },
    )
}

/// R's `qexp(p, rate=1, lower.tail=TRUE, log.p=FALSE)` — exponential quantile.
pub unsafe fn do_qexp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "rate", "lower.tail", "log.p"],
        1.0,
        0.0,
        |p, rate, _, lower_tail, log_p| {
            crate::dist::exponential::qexp_inner(p, 1.0 / rate, lower_tail, log_p)
        },
    )
}

// ---------------------------------------------------------------------------
// Distribution functions: gamma, beta, t, chisq, cauchy, weibull, f, nbinom, geom
// ---------------------------------------------------------------------------

/// R's `dgamma(x, shape, rate=1, scale=1/rate, log=FALSE)` — gamma density.
pub unsafe fn do_dgamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = dist_match(args, &["x", "shape", "rate", "scale", "log"]);
        let shape = real_or_default(m[1], 1.0);
        let scale = dist_scale(m[2], m[3]);
        let give_log = logical_arg(m[4], false);
        map_real_distribution(m[0], |x| {
            crate::dist::gamma::dgamma_inner(x, shape, scale, give_log)
        })
    }
}

/// R's `pgamma(q, shape, rate=1, scale=1/rate, lower.tail=TRUE, log.p=FALSE)` — gamma CDF.
pub unsafe fn do_pgamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = dist_match(
            args,
            &["q", "shape", "rate", "scale", "lower.tail", "log.p"],
        );
        let shape = real_or_default(m[1], 1.0);
        let scale = dist_scale(m[2], m[3]);
        let lower_tail = logical_arg(m[4], true);
        let log_p = logical_arg(m[5], false);
        map_real_distribution(m[0], |q| {
            crate::dist::gamma::pgamma_inner(q, shape, scale, lower_tail, log_p)
        })
    }
}

/// R's `qgamma(p, shape, rate=1, scale=1/rate, lower.tail=TRUE, log.p=FALSE)` — gamma quantile.
pub unsafe fn do_qgamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = dist_match(
            args,
            &["p", "shape", "rate", "scale", "lower.tail", "log.p"],
        );
        let shape = real_or_default(m[1], 1.0);
        let scale = dist_scale(m[2], m[3]);
        let lower_tail = logical_arg(m[4], true);
        let log_p = logical_arg(m[5], false);
        map_real_distribution(m[0], |p| {
            crate::dist::gamma::qgamma_inner(p, shape, scale, lower_tail, log_p)
        })
    }
}

/// R's `dbeta(x, shape1, shape2, log=FALSE)` — beta density.
pub unsafe fn do_dbeta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(
        args,
        &["x", "shape1", "shape2", "log"],
        1.0,
        1.0,
        crate::dist::beta::dbeta_inner,
    )
}

/// R's `pbeta(q, shape1, shape2, lower.tail=TRUE, log.p=FALSE)` — beta CDF.
pub unsafe fn do_pbeta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "shape1", "shape2", "lower.tail", "log.p"],
        1.0,
        1.0,
        |q, shape1, shape2, lower_tail, log_p| {
            crate::dist::beta::pbeta_inner(q, shape1, shape2, lower_tail, log_p)
        },
    )
}

/// R's `qbeta(p, shape1, shape2, lower.tail=TRUE, log.p=FALSE)` — beta quantile.
pub unsafe fn do_qbeta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "shape1", "shape2", "lower.tail", "log.p"],
        1.0,
        1.0,
        |p, shape1, shape2, lower_tail, log_p| {
            crate::dist::beta::qbeta_inner(p, shape1, shape2, lower_tail, log_p)
        },
    )
}

/// R's `dt(x, df, log=FALSE)` — t density.
pub unsafe fn do_dt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(args, &["x", "df", "log"], 1.0, 0.0, |x, df, _, give_log| {
        crate::dist::t_dist::dt_inner(x, df, give_log)
    })
}

/// R's `pt(q, df, lower.tail=TRUE, log.p=FALSE)` — t CDF.
pub unsafe fn do_pt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "df", "lower.tail", "log.p"],
        1.0,
        0.0,
        |q, df, _, lower_tail, log_p| crate::dist::t_dist::pt_inner(q, df, lower_tail, log_p),
    )
}

/// R's `qt(p, df, lower.tail=TRUE, log.p=FALSE)` — t quantile.
pub unsafe fn do_qt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "df", "lower.tail", "log.p"],
        1.0,
        0.0,
        |p, df, _, lower_tail, log_p| crate::dist::t_dist::qt_inner(p, df, lower_tail, log_p),
    )
}

/// R's `dchisq(x, df, log=FALSE)` — chi-squared density.
pub unsafe fn do_dchisq(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(args, &["x", "df", "log"], 1.0, 0.0, |x, df, _, give_log| {
        crate::dist::chisq::dchisq_inner(x, df, give_log)
    })
}

/// R's `pchisq(q, df, lower.tail=TRUE, log.p=FALSE)` — chi-squared CDF.
pub unsafe fn do_pchisq(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "df", "lower.tail", "log.p"],
        1.0,
        0.0,
        |q, df, _, lower_tail, log_p| crate::dist::chisq::pchisq_inner(q, df, lower_tail, log_p),
    )
}

/// R's `qchisq(p, df, lower.tail=TRUE, log.p=FALSE)` — chi-squared quantile.
pub unsafe fn do_qchisq(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "df", "lower.tail", "log.p"],
        1.0,
        0.0,
        |p, df, _, lower_tail, log_p| crate::dist::chisq::qchisq_inner(p, df, lower_tail, log_p),
    )
}

/// R's `dcauchy(x, location=0, scale=1, log=FALSE)` — Cauchy density.
pub unsafe fn do_dcauchy(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(
        args,
        &["x", "location", "scale", "log"],
        0.0,
        1.0,
        crate::dist::cauchy::dcauchy_inner,
    )
}

/// R's `pcauchy(q, location=0, scale=1, lower.tail=TRUE, log.p=FALSE)` — Cauchy CDF.
pub unsafe fn do_pcauchy(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "location", "scale", "lower.tail", "log.p"],
        0.0,
        1.0,
        |q, loc, sc, lower_tail, log_p| {
            crate::dist::cauchy::pcauchy_inner(q, loc, sc, lower_tail, log_p)
        },
    )
}

/// R's `qcauchy(p, location=0, scale=1, lower.tail=TRUE, log.p=FALSE)` — Cauchy quantile.
pub unsafe fn do_qcauchy(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "location", "scale", "lower.tail", "log.p"],
        0.0,
        1.0,
        |p, loc, sc, lower_tail, log_p| {
            crate::dist::cauchy::qcauchy_inner(p, loc, sc, lower_tail, log_p)
        },
    )
}

/// R's `dweibull(x, shape, scale=1, log=FALSE)` — Weibull density.
pub unsafe fn do_dweibull(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(
        args,
        &["x", "shape", "scale", "log"],
        1.0,
        1.0,
        crate::dist::weibull::dweibull_inner,
    )
}

/// R's `pweibull(q, shape, scale=1, lower.tail=TRUE, log.p=FALSE)` — Weibull CDF.
pub unsafe fn do_pweibull(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "shape", "scale", "lower.tail", "log.p"],
        1.0,
        1.0,
        |q, shape, scale, lower_tail, log_p| {
            crate::dist::weibull::pweibull_inner(q, shape, scale, lower_tail, log_p)
        },
    )
}

/// R's `qweibull(p, shape, scale=1, lower.tail=TRUE, log.p=FALSE)` — Weibull quantile.
pub unsafe fn do_qweibull(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "shape", "scale", "lower.tail", "log.p"],
        1.0,
        1.0,
        |p, shape, scale, lower_tail, log_p| {
            crate::dist::weibull::qweibull_inner(p, shape, scale, lower_tail, log_p)
        },
    )
}

/// R's `df(x, df1, df2, log=FALSE)` — F distribution density.
pub unsafe fn do_df(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(
        args,
        &["x", "df1", "df2", "log"],
        1.0,
        1.0,
        crate::dist::f_dist::df_inner,
    )
}

/// R's `pf(q, df1, df2, lower.tail=TRUE, log.p=FALSE)` — F distribution CDF.
pub unsafe fn do_pf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "df1", "df2", "lower.tail", "log.p"],
        1.0,
        1.0,
        |q, df1, df2, lower_tail, log_p| {
            crate::dist::f_dist::pf_inner(q, df1, df2, lower_tail, log_p)
        },
    )
}

/// R's `qf(p, df1, df2, lower.tail=TRUE, log.p=FALSE)` — F distribution quantile.
pub unsafe fn do_qf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "df1", "df2", "lower.tail", "log.p"],
        1.0,
        1.0,
        |p, df1, df2, lower_tail, log_p| {
            crate::dist::f_dist::qf_inner(p, df1, df2, lower_tail, log_p)
        },
    )
}

/// R's `dunif(x, min=0, max=1, log=FALSE)` — uniform density.
pub unsafe fn do_dunif(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(
        args,
        &["x", "min", "max", "log"],
        0.0,
        1.0,
        crate::dist::uniform::dunif_inner,
    )
}

/// R's `punif(q, min=0, max=1, lower.tail=TRUE, log.p=FALSE)` — uniform CDF.
pub unsafe fn do_punif(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "min", "max", "lower.tail", "log.p"],
        0.0,
        1.0,
        |q, min, max, lower_tail, log_p| {
            crate::dist::uniform::punif_inner(q, min, max, lower_tail, log_p)
        },
    )
}

/// R's `qunif(p, min=0, max=1, lower.tail=TRUE, log.p=FALSE)` — uniform quantile.
pub unsafe fn do_qunif(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "min", "max", "lower.tail", "log.p"],
        0.0,
        1.0,
        |p, min, max, lower_tail, log_p| {
            crate::dist::uniform::qunif_inner(p, min, max, lower_tail, log_p)
        },
    )
}

/// Resolve the negative binomial success probability from the mutually
/// exclusive `prob`/`mu` formals (stock R: prob = size/(size+mu)).
unsafe fn nbinom_prob(size: f64, prob_arg: SEXP, mu_arg: SEXP) -> f64 {
    unsafe {
        if !prob_arg.is_null() && prob_arg != R_NilValue() {
            real_or_default(prob_arg, 0.5)
        } else if !mu_arg.is_null() && mu_arg != R_NilValue() {
            size / (size + real_or_default(mu_arg, 0.0))
        } else {
            0.5
        }
    }
}

/// R's `dnbinom(x, size, prob, mu, log=FALSE)` — negative binomial density.
pub unsafe fn do_dnbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = dist_match(args, &["x", "size", "prob", "mu", "log"]);
        let size = real_or_default(m[1], 1.0);
        let prob = nbinom_prob(size, m[2], m[3]);
        let give_log = logical_arg(m[4], false);
        map_real_distribution(m[0], |x| {
            crate::dist::nbinom::dnbinom_inner(x, size, prob, give_log)
        })
    }
}

/// R's `pnbinom(q, size, prob, mu, lower.tail=TRUE, log.p=FALSE)` — negative binomial CDF.
pub unsafe fn do_pnbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = dist_match(args, &["q", "size", "prob", "mu", "lower.tail", "log.p"]);
        let size = real_or_default(m[1], 1.0);
        let prob = nbinom_prob(size, m[2], m[3]);
        let lower_tail = logical_arg(m[4], true);
        let log_p = logical_arg(m[5], false);
        map_real_distribution(m[0], |q| {
            crate::dist::nbinom::pnbinom_inner(q, size, prob, lower_tail, log_p)
        })
    }
}

/// R's `qnbinom(p, size, prob, mu, lower.tail=TRUE, log.p=FALSE)` — negative binomial quantile.
pub unsafe fn do_qnbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = dist_match(args, &["p", "size", "prob", "mu", "lower.tail", "log.p"]);
        let size = real_or_default(m[1], 1.0);
        let prob = nbinom_prob(size, m[2], m[3]);
        let lower_tail = logical_arg(m[4], true);
        let log_p = logical_arg(m[5], false);
        map_real_distribution(m[0], |p| {
            crate::dist::nbinom::qnbinom_inner(p, size, prob, lower_tail, log_p)
        })
    }
}

/// R's `dgeom(x, prob, log=FALSE)` — geometric density.
pub unsafe fn do_dgeom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(
        args,
        &["x", "prob", "log"],
        0.5,
        0.0,
        |x, p, _, give_log| crate::dist::geometric::dgeom_inner(x, p, give_log),
    )
}

/// R's `pgeom(q, prob, lower.tail=TRUE, log.p=FALSE)` — geometric CDF.
pub unsafe fn do_pgeom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "prob", "lower.tail", "log.p"],
        0.5,
        0.0,
        |q, p, _, lower_tail, log_p| crate::dist::geometric::pgeom_inner(q, p, lower_tail, log_p),
    )
}

/// R's `qgeom(p, prob, lower.tail=TRUE, log.p=FALSE)` — geometric quantile.
pub unsafe fn do_qgeom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "prob", "lower.tail", "log.p"],
        0.5,
        0.0,
        |p, prob, _, lower_tail, log_p| {
            crate::dist::geometric::qgeom_inner(p, prob, lower_tail, log_p)
        },
    )
}

// ---------------------------------------------------------------------------
// Distribution functions: lnorm, logistic, signrank, wilcox, hyper, tukey
// ---------------------------------------------------------------------------

/// R's `dlnorm(x, meanlog=0, sdlog=1, log=FALSE)` — lognormal density.
pub unsafe fn do_dlnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(
        args,
        &["x", "meanlog", "sdlog", "log"],
        0.0,
        1.0,
        crate::dist::lnorm::dlnorm_inner,
    )
}

/// R's `plnorm(q, meanlog=0, sdlog=1, lower.tail=TRUE, log.p=FALSE)` — lognormal CDF.
pub unsafe fn do_plnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "meanlog", "sdlog", "lower.tail", "log.p"],
        0.0,
        1.0,
        |q, meanlog, sdlog, lower_tail, log_p| {
            crate::dist::lnorm::plnorm_inner(q, meanlog, sdlog, lower_tail, log_p)
        },
    )
}

/// R's `qlnorm(p, meanlog=0, sdlog=1, lower.tail=TRUE, log.p=FALSE)` — lognormal quantile.
pub unsafe fn do_qlnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "meanlog", "sdlog", "lower.tail", "log.p"],
        0.0,
        1.0,
        |p, meanlog, sdlog, lower_tail, log_p| {
            crate::dist::lnorm::qlnorm_inner(p, meanlog, sdlog, lower_tail, log_p)
        },
    )
}

/// R's `dlogis(x, location=0, scale=1, log=FALSE)` — logistic density.
pub unsafe fn do_dlogis(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(
        args,
        &["x", "location", "scale", "log"],
        0.0,
        1.0,
        |x, location, scale, give_log| {
            crate::dist::logistic::dlogis_inner(x, location, scale, give_log)
        },
    )
}

/// R's `plogis(q, location=0, scale=1, lower.tail=TRUE, log.p=FALSE)` — logistic CDF.
pub unsafe fn do_plogis(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "location", "scale", "lower.tail", "log.p"],
        0.0,
        1.0,
        |q, location, scale, lower_tail, log_p| {
            crate::dist::logistic::plogis_inner(q, location, scale, lower_tail, log_p)
        },
    )
}

/// R's `qlogis(p, location=0, scale=1, lower.tail=TRUE, log.p=FALSE)` — logistic quantile.
pub unsafe fn do_qlogis(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "location", "scale", "lower.tail", "log.p"],
        0.0,
        1.0,
        |p, location, scale, lower_tail, log_p| {
            crate::dist::logistic::qlogis_inner(p, location, scale, lower_tail, log_p)
        },
    )
}

/// R's `dsignrank(x, n, log=FALSE)` — Wilcoxon signed rank density.
pub unsafe fn do_dsignrank(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(args, &["x", "n", "log"], 1.0, 0.0, |x, n, _, give_log| {
        crate::dist::signrank::dsignrank_inner(x, n, give_log)
    })
}

/// R's `psignrank(q, n, lower.tail=TRUE, log.p=FALSE)` — Wilcoxon signed rank CDF.
pub unsafe fn do_psignrank(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "n", "lower.tail", "log.p"],
        1.0,
        0.0,
        |q, n, _, lower_tail, log_p| {
            crate::dist::signrank::psignrank_inner(q, n, lower_tail, log_p)
        },
    )
}

/// R's `qsignrank(p, n, lower.tail=TRUE, log.p=FALSE)` — Wilcoxon signed rank quantile.
pub unsafe fn do_qsignrank(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "n", "lower.tail", "log.p"],
        1.0,
        0.0,
        |p, n, _, lower_tail, log_p| {
            crate::dist::signrank::qsignrank_inner(p, n, lower_tail, log_p)
        },
    )
}

/// R's `dwilcox(x, m, n, log=FALSE)` — Wilcoxon rank sum density.
pub unsafe fn do_dwilcox(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(
        args,
        &["x", "m", "n", "log"],
        1.0,
        1.0,
        crate::dist::wilcox::dwilcox_inner,
    )
}

/// R's `pwilcox(q, m, n, lower.tail=TRUE, log.p=FALSE)` — Wilcoxon rank sum CDF.
pub unsafe fn do_pwilcox(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "m", "n", "lower.tail", "log.p"],
        1.0,
        1.0,
        crate::dist::wilcox::pwilcox_inner,
    )
}

/// R's `qwilcox(p, m, n, lower.tail=TRUE, log.p=FALSE)` — Wilcoxon rank sum quantile.
pub unsafe fn do_qwilcox(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "m", "n", "lower.tail", "log.p"],
        1.0,
        1.0,
        crate::dist::wilcox::qwilcox_inner,
    )
}

/// R's `dhyper(x, m, n, k, log=FALSE)` — hypergeometric density (4 params).
pub unsafe fn do_dhyper(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_tertiary_with_log(
        args,
        &["x", "m", "n", "k", "log"],
        1.0,
        1.0,
        1.0,
        crate::dist::hypergeometric::dhyper_inner,
    )
}

/// R's `phyper(q, m, n, k, lower.tail=TRUE, log.p=FALSE)` — hypergeometric CDF (4 params).
pub unsafe fn do_phyper(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_tertiary_with_tail_log(
        args,
        &["q", "m", "n", "k", "lower.tail", "log.p"],
        1.0,
        1.0,
        1.0,
        |q, m, n, k, lower_tail, log_p| {
            crate::dist::hypergeometric::phyper_inner(q, m, n, k, lower_tail, log_p)
        },
    )
}

/// R's `qhyper(p, m, n, k, lower.tail=TRUE, log.p=FALSE)` — hypergeometric quantile (4 params).
pub unsafe fn do_qhyper(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_tertiary_with_tail_log(
        args,
        &["p", "m", "n", "k", "lower.tail", "log.p"],
        1.0,
        1.0,
        1.0,
        |p, m, n, k, lower_tail, log_p| {
            crate::dist::hypergeometric::qhyper_inner(p, m, n, k, lower_tail, log_p)
        },
    )
}

/// R's `ptukey(q, nmeans, df, nranges=1, lower.tail=TRUE, log.p=FALSE)` —
/// Studentized range CDF (nranges defaults to 1).
pub unsafe fn do_ptukey(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["q", "nmeans", "df", "lower.tail", "log.p"],
        2.0,
        1.0,
        |q, nmeans, df, lower_tail, log_p| {
            crate::dist::tukey::ptukey_inner(q, 1.0, nmeans, df, lower_tail, log_p)
        },
    )
}

/// R's `qtukey(p, nmeans, df, nranges=1, lower.tail=TRUE, log.p=FALSE)` —
/// Studentized range quantile (nranges defaults to 1).
pub unsafe fn do_qtukey(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(
        args,
        &["p", "nmeans", "df", "lower.tail", "log.p"],
        2.0,
        1.0,
        |p, nmeans, df, lower_tail, log_p| {
            crate::dist::tukey::qtukey_inner(p, 1.0, nmeans, df, lower_tail, log_p)
        },
    )
}

/// R's `dmultinom(x, prob, log=FALSE)` — multinomial probability.
pub unsafe fn do_dmultinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = dist_match(args, &["x", "prob", "log"]);
        let x_arg = m[0];
        let prob_arg = m[1];

        if x_arg.is_null() || prob_arg.is_null() {
            return R_NilValue();
        }

        let nx = if x_arg == R_NilValue() {
            0
        } else {
            XLENGTH(x_arg)
        };
        let np = if prob_arg == R_NilValue() {
            0
        } else {
            XLENGTH(prob_arg)
        };
        if nx != np {
            base_error("x[] and prob[] must be equal length vectors.");
        }
        let give_log = logical_arg(m[2], false);

        // Collect x values
        let mut xv: Vec<f64> = Vec::with_capacity(nx as usize);
        for i in 0..nx {
            xv.push(elt_real_safe(x_arg, i));
        }

        // Collect and validate prob values
        let mut pv: Vec<f64> = Vec::with_capacity(np as usize);
        let mut prob_sum = 0.0;
        for i in 0..np {
            let p = elt_real_safe(prob_arg, i);
            if !p.is_finite() || p < 0.0 {
                base_error("probabilities must be finite, non-negative and not all 0");
            }
            prob_sum += p;
            pv.push(p);
        }
        if prob_sum <= 0.0 {
            base_error("probabilities must be finite, non-negative and not all 0");
        }
        for p in &mut pv {
            *p /= prob_sum;
        }

        // dmultinom: log-probability of multinomial outcome
        // Uses lgammafn(x+1) for log-factorial terms
        let k = xv.len().min(pv.len());
        let n_total: f64 = xv.iter().sum();

        let mut log_prob = crate::special::gamma::lgammafn(n_total + 1.0);
        for i in 0..k {
            log_prob -= crate::special::gamma::lgammafn(xv[i] + 1.0);
            if pv[i] > 0.0 {
                log_prob += xv[i] * pv[i].ln();
            } else if xv[i] > 0.0 {
                log_prob = f64::NEG_INFINITY;
            }
        }

        let result = if give_log { log_prob } else { log_prob.exp() };
        Rf_ScalarReal(result)
    }
}

/// Generic vectorized distribution function with 3 extra parameters plus `log`.
fn do_dist_tertiary_with_log(
    args: SEXP,
    names: &[&str],
    default_p1: f64,
    default_p2: f64,
    default_p3: f64,
    f: fn(f64, f64, f64, f64, bool) -> f64,
) -> SEXP {
    unsafe {
        let m = dist_match(args, names);
        let x = m[0];
        let p1 = real_or_default(m[1], default_p1);
        let p2 = real_or_default(m[2], default_p2);
        let p3 = real_or_default(m[3], default_p3);
        let give_log = logical_arg(m[names.len() - 1], false);
        map_real_distribution(x, |x| f(x, p1, p2, p3, give_log))
    }
}
/// Generic vectorized distribution function with 3 extra parameters plus
/// `lower.tail`/`log.p`.
fn do_dist_tertiary_with_tail_log(
    args: SEXP,
    names: &[&str],
    default_p1: f64,
    default_p2: f64,
    default_p3: f64,
    f: fn(f64, f64, f64, f64, bool, bool) -> f64,
) -> SEXP {
    unsafe {
        let m = dist_match(args, names);
        let x = m[0];
        let p1 = real_or_default(m[1], default_p1);
        let p2 = real_or_default(m[2], default_p2);
        let p3 = real_or_default(m[3], default_p3);
        let lower_tail = logical_arg(m[names.len() - 2], true);
        let log_p = logical_arg(m[names.len() - 1], false);
        map_real_distribution(x, |x| f(x, p1, p2, p3, lower_tail, log_p))
    }
}

fn do_dist_unary_with_log(
    args: SEXP,
    names: &[&str],
    default_p1: f64,
    default_p2: f64,
    f: fn(f64, f64, f64, bool) -> f64,
) -> SEXP {
    unsafe {
        let m = dist_match(args, names);
        let p1 = real_or_default(m[1], default_p1);
        let p2 = real_or_default(m[2], default_p2);
        let give_log = logical_arg(m[names.len() - 1], false);
        map_real_distribution(m[0], |x| f(x, p1, p2, give_log))
    }
}

fn do_dist_unary_with_tail_log(
    args: SEXP,
    names: &[&str],
    default_p1: f64,
    default_p2: f64,
    f: fn(f64, f64, f64, bool, bool) -> f64,
) -> SEXP {
    unsafe {
        let m = dist_match(args, names);
        let p1 = real_or_default(m[1], default_p1);
        let p2 = real_or_default(m[2], default_p2);
        let lower_tail = logical_arg(m[names.len() - 2], true);
        let log_p = logical_arg(m[names.len() - 1], false);
        map_real_distribution(m[0], |x| f(x, p1, p2, lower_tail, log_p))
    }
}

unsafe fn map_real_distribution(mut x: SEXP, f: impl Fn(f64) -> f64) -> SEXP {
    unsafe {
        if x.is_null() {
            return R_NilValue();
        }
        if x == R_NilValue() {
            x = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
            if x.is_null() {
                return R_NilValue();
            }
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        if n == 0 {
            return result;
        }
        let dst = REAL(result);
        for i in 0..n {
            *dst.add(i as usize) = f(elt_real_safe(x, i));
        }
        result
    }
}
