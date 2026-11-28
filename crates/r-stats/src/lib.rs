//! Statistics builtins exactly as GNU's stats package organizes them —
//! the dpq distribution surface, random samplers, sampling algorithms,
//! statistical summaries, and the loess/predict modeling chain, extracted
//! from the flat registry as the second domain module.

#![forbid(unsafe_code)]
use r_core::prim::pairlist_parts;
use r_core::prim::*;
use r_core::primitive_ids::*;
use r_core::attrib::{get_attrib, string_vec};
use r_core::eval::{eval, force_if_promise, EvalError, EvalResult, RSignal};
use r_core::heap::Heap;
use r_core::id::ValueId;
use r_core::intern::SymbolInterner;
use r_core::object::RObject;

// Trigonometric and hyperbolic family — real inputs produce real outputs
// exactly as the other unary math; complex inputs evaluate through the
// standard real-part compositions GNU's complex trig derives from.

/// Density/distribution family over nmath's pure-Rust cores — GNU's exact
/// positional+named argument resolution with scalar parameter defaults,
/// vectorized over the first argument exactly as GNU's DPQ dispatch.
pub mod linpack;
pub mod loessf;

pub fn dpq_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    which: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut named: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    while let Some((car, cdr, tag)) = pairlist_parts(heap, cursor) {
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => {
                named.push((interner.symbol_name(*name).to_string(), value));
            }
            _ => positional.push(value),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    // GNU's argument matching: exact first, then a unique partial prefix
    // exactly as matchArgs formal resolution.
    let lookup = |name: &str, position: usize| -> Option<ValueId> {
        named
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
            .or_else(|| {
                let partials: Vec<ValueId> = named
                    .iter()
                    .filter(|(n, _)| !n.is_empty() && name.starts_with(n.as_str()))
                    .map(|(_, v)| *v)
                    .collect();
                if partials.len() == 1 {
                    Some(partials[0])
                } else {
                    None
                }
            })
            .or_else(|| positional.get(position).copied())
    };
    let real_arg = |name: &str, position: usize, default: f64| -> Result<f64, RSignal> {
        match lookup(name, position) {
            Some(v) => first_real(heap, v).map_err(RSignal::from),
            None => Ok(default),
        }
    };
    let logical_of = |heap: &Heap, v: ValueId| -> Result<bool, RSignal> {
        match heap.object(v) {
            Some(RObject::Logical(vals)) => Ok(vals.first().copied().unwrap_or(0) != 0),
            Some(RObject::Real(vals)) => Ok(vals.first().copied().unwrap_or(0.0) != 0.0),
            Some(RObject::Integer(vals)) => Ok(vals.first().copied().unwrap_or(0) != 0),
            _ => Err(EvalError::TypeError("logical flag expected").into()),
        }
    };
    let logical_flag = |names: &[&str], position: usize, default: bool| -> Result<bool, RSignal> {
        for name in names {
            if let Some(v) = named.iter().find(|(n, _)| n == name).map(|(_, v)| *v) {
                return logical_of(heap, v);
            }
            // GNU's unique partial prefix matching over the formal's name.
            let partials: Vec<ValueId> = named
                .iter()
                .filter(|(n, _)| !n.is_empty() && name.starts_with(n.as_str()))
                .map(|(_, v)| *v)
                .collect();
            if partials.len() == 1 {
                return logical_of(heap, partials[0]);
            }
        }
        if let Some(v) = positional.get(position)
            && !named.iter().any(|(n, _)| names.contains(&n.as_str())) {
                return logical_of(heap, *v);
            }
        Ok(default)
    };
    let x_value = lookup("x", 0)
        .or_else(|| lookup("q", 0))
        .or_else(|| lookup("p", 0))
        .ok_or(EvalError::TypeError("dpq requires a first argument"))?;
    let xs: Vec<f64> = match heap.object(x_value) {
        Some(RObject::Real(vals)) => vals.clone(),
        Some(RObject::Integer(vals)) => vals.iter().map(|&v| if v == i32::MIN { f64::NAN } else { v as f64 }).collect(),
        Some(RObject::Logical(vals)) => vals.iter().map(|&v| if v == i32::MIN { f64::NAN } else { v as f64 }).collect(),
        _ => return Err(EvalError::TypeError("numeric argument expected").into()),
    };
    let (p1n, p1d, p2n, p2d): (&str, f64, &str, f64) = match which {
        DNORM | PNORM | QNORM => ("mean", 0.0, "sd", 1.0),
        DPOIS | PPOIS => ("lambda", f64::NAN, "", 0.0),
        DBINOM | PBINOM => ("size", f64::NAN, "prob", f64::NAN),
        PBETA | DBETA | QBETA => ("shape1", f64::NAN, "shape2", f64::NAN),
        DT | PT | QT => ("df", f64::NAN, "", 0.0),
        DF | PF | QF => ("df1", f64::NAN, "df2", f64::NAN),
        DCAUCHY | PCAUCHY | QCAUCHY => ("location", 0.0, "scale", 1.0),
        DCHISQ | PCHISQ | QCHISQ => ("df", f64::NAN, "", 0.0),
        DEXP | PEXP | QEXP => ("rate", 1.0, "", 0.0),
        DGAMMA | PGAMMA | QGAMMA => ("shape", f64::NAN, "rate", 1.0),
        DGEOM | PGEOM | QGEOM => ("prob", f64::NAN, "", 0.0),
        DNBINOM | PNBINOM | QNBINOM => ("size", f64::NAN, "prob", f64::NAN),
        DWEIBULL | PWEIBULL | QWEIBULL => ("shape", f64::NAN, "scale", 1.0),
        QPOIS => ("lambda", f64::NAN, "", 0.0),
        QBINOM => ("size", f64::NAN, "prob", f64::NAN),
        DUNIF | PUNIF | QUNIF => ("min", 0.0, "max", 1.0),
        _ => unreachable!(),
    };
    let param1 = real_arg(p1n, 1, p1d)?;
    if param1.is_nan() && (matches!(which, DPOIS | PPOIS) || lookup("lambda", 1).is_none() && matches!(which, DPOIS | PPOIS)) {
        return Err(EvalError::TypeError("dpq requires lambda").into());
    }
    let param2 = if p2n.is_empty() { 0.0 } else { real_arg(p2n, 2, p2d)? };
    let log_flag = logical_flag(&["log", "log.p"], 3, false)?;
    let lower_flag = logical_flag(&["lower.tail"], 3, true)?;
    let out: Vec<f64> = xs
        .iter()
        .map(|&x| {
            let x = if x.is_nan() { f64::NAN } else { x };
            match which {
                DNORM => r_math::dist::normal::dnorm4_inner(x, param1, param2, log_flag),
                PNORM => r_math::dist::normal::pnorm5_inner(x, param1, param2, lower_flag, log_flag),
                QNORM => r_math::dist::normal::qnorm5_inner(x, param1, param2, lower_flag, log_flag),
                DPOIS => r_math::dist::poisson::dpois_inner(x, param1, log_flag),
                PPOIS => r_math::dist::poisson::ppois_inner(x, param1, lower_flag, log_flag),
                DBINOM => r_math::dist::binomial::dbinom_inner(x, param1, param2, log_flag),
                PBINOM => r_math::dist::binomial::pbinom_inner(x, param1, param2, lower_flag, log_flag),
                DBETA => r_math::dist::beta::dbeta_inner(x, param1, param2, log_flag),
                PBETA => r_math::dist::beta::pbeta_inner(x, param1, param2, lower_flag, log_flag),
                QBETA => r_math::dist::beta::qbeta_inner(x, param1, param2, lower_flag, log_flag),
                DT => r_math::dist::t_dist::dt_inner(x, param1, log_flag),
                PT => r_math::dist::t_dist::pt_inner(x, param1, lower_flag, log_flag),
                QT => r_math::dist::t_dist::qt_inner(x, param1, lower_flag, log_flag),
                DF => r_math::dist::f_dist::df_inner(x, param1, param2, log_flag),
                PF => r_math::dist::f_dist::pf_inner(x, param1, param2, lower_flag, log_flag),
                QF => r_math::dist::f_dist::qf_inner(x, param1, param2, lower_flag, log_flag),
                DCAUCHY => r_math::dist::cauchy::dcauchy_inner(x, param1, param2, log_flag),
                PCAUCHY => r_math::dist::cauchy::pcauchy_inner(x, param1, param2, lower_flag, log_flag),
                QCAUCHY => r_math::dist::cauchy::qcauchy_inner(x, param1, param2, lower_flag, log_flag),
                DCHISQ => r_math::dist::chisq::dchisq_inner(x, param1, log_flag),
                PCHISQ => r_math::dist::chisq::pchisq_inner(x, param1, lower_flag, log_flag),
                QCHISQ => r_math::dist::chisq::qchisq_inner(x, param1, lower_flag, log_flag),
                DEXP => r_math::dist::exponential::dexp_inner(x, 1.0 / param1, log_flag),
                // rate inverts to the core's scale exactly as the d-variant.
                PEXP => r_math::dist::exponential::pexp_inner(x, 1.0 / param1, lower_flag, log_flag),
                QEXP => r_math::dist::exponential::qexp_inner(x, 1.0 / param1, lower_flag, log_flag),
                // GNU's gamma exposes rate with scale = 1/rate exactly as
                // the exponential inversion — the core wants scale.
                DGAMMA => r_math::dist::gamma::dgamma_inner(x, param1, 1.0 / param2, log_flag),
                PGAMMA => r_math::dist::gamma::pgamma_inner(x, param1, 1.0 / param2, lower_flag, log_flag),
                QGAMMA => r_math::dist::gamma::qgamma_inner(x, param1, 1.0 / param2, lower_flag, log_flag),
                DGEOM => r_math::dist::geometric::dgeom_inner(x, param1, log_flag),
                PGEOM => r_math::dist::geometric::pgeom_inner(x, param1, lower_flag, log_flag),
                QGEOM => r_math::dist::geometric::qgeom_inner(x, param1, lower_flag, log_flag),
                DNBINOM => r_math::dist::nbinom::dnbinom_inner(x, param1, param2, log_flag),
                PNBINOM => r_math::dist::nbinom::pnbinom_inner(x, param1, param2, lower_flag, log_flag),
                QNBINOM => r_math::dist::nbinom::qnbinom_inner(x, param1, param2, lower_flag, log_flag),
                DWEIBULL => r_math::dist::weibull::dweibull_inner(x, param1, param2, log_flag),
                PWEIBULL => r_math::dist::weibull::pweibull_inner(x, param1, param2, lower_flag, log_flag),
                QWEIBULL => r_math::dist::weibull::qweibull_inner(x, param1, param2, lower_flag, log_flag),
                QPOIS => r_math::dist::poisson::qpois_inner(x, param1, lower_flag, log_flag),
                QBINOM => r_math::dist::binomial::qbinom_inner(x, param1, param2, lower_flag, log_flag),
                DUNIF => r_math::dist::uniform::dunif_inner(x, param1, param2, log_flag),
                PUNIF => r_math::dist::uniform::punif_inner(x, param1, param2, lower_flag, log_flag),
                QUNIF => r_math::dist::uniform::qunif_inner(x, param1, param2, lower_flag, log_flag),
                _ => unreachable!(),
            }
        })
        .collect();
    Ok(heap.alloc(RObject::Real(out)))
}

/// qhyper(p, m, n, k) — three-parameter hypergeometric quantile, outside
/// the two-parameter dpq machinery.
pub fn qhyper_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let bound = bind_formals(heap, interner, args, caller_env, &["x", "m", "n", "k", "lower.tail", "log.p"])?;
    let positional: Vec<ValueId> = bound.iter().map(|s| s.unwrap_or(ValueId::NIL)).collect();
    let x = bound[0].ok_or(EvalError::TypeError("qhyper requires input"))?;
    let xs = numeric_vector(heap, x);
    let m = positional.get(1).and_then(|v| first_real(heap, *v).ok()).unwrap_or(f64::NAN);
    let n = positional.get(2).and_then(|v| first_real(heap, *v).ok()).unwrap_or(f64::NAN);
    let k = positional.get(3).and_then(|v| first_real(heap, *v).ok()).unwrap_or(f64::NAN);
    let present = |v: &ValueId| if *v == ValueId::NIL { None } else { logical_or_na(heap, *v) };
    let lower = positional.get(4).and_then(present).unwrap_or(true);
    let logp = positional.get(5).and_then(present).unwrap_or(false);
    let out: Vec<f64> = xs
        .iter()
        .map(|&v| r_math::dist::hypergeometric::qhyper_inner(v, m, n, k, lower, logp))
        .collect();
    Ok(heap.alloc(RObject::Real(out)))
}

/// phyper(x, m, n, k) — three-parameter hypergeometric CDF, mirroring
/// the quantile primitive exactly with the sign conventions inverted.
pub fn phyper_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let bound = bind_formals(heap, interner, args, caller_env, &["x", "m", "n", "k", "lower.tail", "log.p"])?;
    let positional: Vec<ValueId> = bound.iter().map(|s| s.unwrap_or(ValueId::NIL)).collect();
    let x = bound[0].ok_or(EvalError::TypeError("phyper requires input"))?;
    let xs = numeric_vector(heap, x);
    let m = positional.get(1).and_then(|v| first_real(heap, *v).ok()).unwrap_or(f64::NAN);
    let n = positional.get(2).and_then(|v| first_real(heap, *v).ok()).unwrap_or(f64::NAN);
    let k = positional.get(3).and_then(|v| first_real(heap, *v).ok()).unwrap_or(f64::NAN);
    let present = |v: &ValueId| if *v == ValueId::NIL { None } else { logical_or_na(heap, *v) };
    let lower = positional.get(4).and_then(present).unwrap_or(true);
    let logp = positional.get(5).and_then(present).unwrap_or(false);
    let out: Vec<f64> = xs
        .iter()
        .map(|&v| r_math::dist::hypergeometric::phyper_inner(v, m, n, k, lower, logp))
        .collect();
    Ok(heap.alloc(RObject::Real(out)))
}

/// dhyper(x, m, n, k) — three-parameter hypergeometric density.
pub fn dhyper_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let bound = bind_formals(heap, interner, args, caller_env, &["x", "m", "n", "k", "log"])?;
    let positional: Vec<ValueId> = bound.iter().map(|s| s.unwrap_or(ValueId::NIL)).collect();
    let x = bound[0].ok_or(EvalError::TypeError("dhyper requires input"))?;
    let xs = numeric_vector(heap, x);
    let m = positional.get(1).and_then(|v| first_real(heap, *v).ok()).unwrap_or(f64::NAN);
    let n = positional.get(2).and_then(|v| first_real(heap, *v).ok()).unwrap_or(f64::NAN);
    let k = positional.get(3).and_then(|v| first_real(heap, *v).ok()).unwrap_or(f64::NAN);
    let present4 = |v: &ValueId| if *v == ValueId::NIL { None } else { logical_or_na(heap, *v) };
    let logp = positional.get(4).and_then(present4).unwrap_or(false);
    let out: Vec<f64> = xs
        .iter()
        .map(|&v| r_math::dist::hypergeometric::dhyper_inner(v, m, n, k, logp))
        .collect();
    Ok(heap.alloc(RObject::Real(out)))
}

/// ptukey(q, nmeans, df, nranges) — the studentized range CDF; the core
/// takes nranges ahead of nmeans, inverted at this boundary.
pub fn ptukey_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let bound = bind_formals(heap, interner, args, caller_env, &["q", "nmeans", "df", "nranges", "lower.tail", "log.p"])?;
    let positional: Vec<ValueId> = bound.iter().map(|s| s.unwrap_or(ValueId::NIL)).collect();
    let x = bound[0].ok_or(EvalError::TypeError("ptukey requires input"))?;
    let xs = numeric_vector(heap, x);
    let nmeans = positional.get(1).and_then(|v| first_real(heap, *v).ok()).unwrap_or(f64::NAN);
    let df = positional.get(2).and_then(|v| first_real(heap, *v).ok()).unwrap_or(f64::NAN);
    let nranges = positional.get(3).and_then(|v| first_real(heap, *v).ok()).unwrap_or(1.0);
    let present = |v: &ValueId| if *v == ValueId::NIL { None } else { logical_or_na(heap, *v) };
    let lower = positional.get(4).and_then(present).unwrap_or(true);
    let logp = positional.get(5).and_then(present).unwrap_or(false);
    let out: Vec<f64> = xs
        .iter()
        .map(|&v| r_math::dist::tukey::ptukey_inner(v, nranges, nmeans, df, lower, logp))
        .collect();
    Ok(heap.alloc(RObject::Real(out)))
}

/// GNU's weighted sampling exactly as random.c: FixupProb validation and
/// normalization, then ProbSampleReplace (descending heapsort cumulative
/// scan) or walker_ProbSampleReplace (alias method beyond 200 dense
/// categories) with replacement, ProbSampleNoReplace (totalmass depletion)
/// without. The draw sequence consumes the heap stream exactly as GNU.
pub fn weighted_sample_indices_pure(
    heap: &mut Heap,
    probs: &[f64],
    size: usize,
    replace: bool,
) -> Result<Vec<usize>, RSignal> {
    let n = probs.len();
    let mut p = probs.to_vec();
    // FixupProb exactly: validation order NA then negative then positive
    // count, normalization to sum one.
    let mut sum = 0.0;
    let mut npos = 0usize;
    for value in p.iter() {
        if !value.is_finite() {
            return Err(RSignal::Error(EvalError::Condition("NA in probability vector".to_string())));
        }
        if *value < 0.0 {
            return Err(RSignal::Error(EvalError::Condition("negative probability".to_string())));
        }
        if *value > 0.0 {
            npos += 1;
            sum += *value;
        }
    }
    if npos == 0 || (!replace && size > npos) {
        return Err(RSignal::Error(EvalError::Condition("too few positive probabilities".to_string())));
    }
    for value in p.iter_mut() {
        *value /= sum;
    }
    let ans: Vec<usize> = if replace {
        let nc = p.iter().filter(|&&pi| n as f64 * pi > 0.1).count();
        if nc > 200 {
            walker_sample_pure(heap, &mut p, size)
        } else {
            // ProbSampleReplace exactly: descending heapsort with the
            // permutation carried, cumulative in place, linear scan.
            let mut perm: Vec<i32> = (1..=n as i32).collect();
            r_core::support::sort::revsort_with_perm(&mut p, &mut perm, n);
            for i in 1..n {
                p[i] += p[i - 1];
            }
            let nm1 = n.saturating_sub(1);
            let mut out = Vec::with_capacity(size);
            for _ in 0..size {
                let r_u = heap.rng.unif();
                let mut j = nm1;
                #[allow(clippy::needless_range_loop)] // multi-array index arithmetic is the algorithm
                for jj in 0..nm1 {
                    if r_u <= p[jj] {
                        j = jj;
                        break;
                    }
                }
                out.push((perm[j] - 1).max(0) as usize);
            }
            out
        }
    } else {
        // ProbSampleNoReplace exactly: descending heapsort, totalmass
        // depletion with the p/perm shift-down compaction.
        let mut perm: Vec<i32> = (1..=n as i32).collect();
        r_core::support::sort::revsort_with_perm(&mut p, &mut perm, n);
        let mut totalmass = 1.0_f64;
        let mut out = Vec::with_capacity(size);
        for i in 0..size {
            let r_t = totalmass * heap.rng.unif();
            let mut mass = 0.0;
            let mut j = n - 1 - i;
            #[allow(clippy::needless_range_loop)] // multi-array index arithmetic is the algorithm
            for jj in 0..j {
                mass += p[jj];
                if r_t <= mass {
                    j = jj;
                    break;
                }
            }
            out.push((perm[j] - 1).max(0) as usize);
            totalmass -= p[j];
            for k in j..(n - 1 - i) {
                p[k] = p[k + 1];
                perm[k] = perm[k + 1];
            }
        }
        out
    };
    Ok(ans)
}

/// Netlib loess exactly as R's stats package arranges it: count-driven
/// kd-tree partitioning with data-value median splits (ties high),
/// weighted least squares quadratic fits at the cell vertices with
/// tricube neighborhood weights, and cubic Hermite blending of the
/// vertex value/slope pairs for evaluation. Univariate slice of the
/// full algorithm — the smoke oracle matches at 3e-9.
pub fn loess_core_fit(xs: &[f64], ys: &[f64], span: f64, degree: usize) -> Vec<f64> {
    fn vertex_fit(xs: &[f64], ys: &[f64], q: f64, nf: usize, span: f64, degree: usize) -> (f64, f64) {
        let n = xs.len();
        let dist: Vec<f64> = xs.iter().map(|&x| (x - q) * (x - q)).collect();
        let mut idx: Vec<usize> = (0..n).collect();
        idx.sort_by(|&a, &b| dist[a].partial_cmp(&dist[b]).unwrap());
        let rho = dist[idx[nf - 1]] * f64::max(1.0, span);
        if rho <= 0.0 {
            return (0.0, 0.0);
        }
        let p = degree + 1;
        let mut a = vec![vec![0.0f64; p]; p];
        let mut bb = vec![0.0f64; p];
        #[allow(clippy::needless_range_loop)] // multi-array index arithmetic is the algorithm
        for i in 0..nf {
            let k = idx[i];
            let u = (dist[k] / rho).sqrt();
            if u >= 1.0 {
                continue;
            }
            let t = 1.0 - u * u * u;
            let w = t * t * t;
            if w <= 0.0 {
                continue;
            }
            let dx = xs[k] - q;
            let mut powers = vec![1.0f64; 2 * p];
            for t in 1..2 * p {
                powers[t] = powers[t - 1] * dx;
            }
            for r in 0..p {
                bb[r] += w * ys[k] * powers[r];
                for c in 0..p {
                    a[r][c] += w * powers[r + c];
                }
            }
        }
        for col in 0..p {
            let mut piv = col;
            for r in (col + 1)..p {
                if a[r][col].abs() > a[piv][col].abs() {
                    piv = r;
                }
            }
            a.swap(col, piv);
            bb.swap(col, piv);
            let d = a[col][col];
            if d.abs() < 1e-300 {
                continue;
            }
            for r in (col + 1)..p {
                let factor = a[r][col] / d;
                #[allow(clippy::needless_range_loop)] // multi-array index arithmetic is the algorithm
                for c in col..p {
                    a[r][c] -= factor * a[col][c];
                }
                bb[r] -= factor * bb[col];
            }
        }
        let mut coef = vec![0.0f64; p];
        for row in (0..p).rev() {
            let mut acc = bb[row];
            for col in (row + 1)..p {
                acc -= a[row][col] * coef[col];
            }
            coef[row] = if a[row][row].abs() < 1e-300 { 0.0 } else { acc / a[row][row] };
        }
        (coef[0], if coef.len() > 1 { coef[1] } else { 0.0 })
    }

    let n = xs.len();
    if n == 0 {
        return Vec::new();
    }
    let nf = ((n as f64) * span + 1e-5).floor().max((degree + 1) as f64).min(n as f64) as usize;
    // The bounding box expanded exactly as ehg126: mu per side.
    let lo = xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let mu = 0.005 * (hi - lo).max(1e-10 * lo.abs().max(hi.abs()) + 1e-30);
    let (blo, bhi) = (lo - mu, hi + mu);
    let ntol = ((n as f64) * 0.2).floor().max(1.0) as usize;
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| xs[a].partial_cmp(&xs[b]).unwrap());
    let sx: Vec<f64> = order.iter().map(|&i| xs[i]).collect();
    // Count-driven recursive splitting exactly as ehg124: leaf when the
    /// point count drops to ntol; split value is the data value at the
    /// median position floor((count-1)/2) in zero-based slices, the left
    /// son owning the median point itself.
    fn split(sx: &[f64], xlo: f64, xhi: f64, ntol: usize, vertices: &mut Vec<f64>, leaves: &mut Vec<(f64, f64)>) {
        let count = sx.len();
        if count <= ntol || count == 0 {
            leaves.push((xlo, xhi));
            return;
        }
        let m = (count - 1) / 2;
        let xi = sx[m];
        vertices.push(xi);
        split(&sx[..=m], xlo, xi, ntol, vertices, leaves);
        split(&sx[(m + 1)..], xi, xhi, ntol, vertices, leaves);
    }
    let mut vertices: Vec<f64> = vec![blo, bhi];
    let mut leaves: Vec<(f64, f64)> = Vec::new();
    if ntol >= n {
        leaves.push((blo, bhi));
    } else {
        split(&sx, blo, bhi, ntol, &mut vertices, &mut leaves);
    }
    vertices.sort_by(|a, b| a.partial_cmp(b).unwrap());
    vertices.dedup();
    let fits: Vec<(f64, f64)> = vertices.iter().map(|&q| vertex_fit(xs, ys, q, nf, span, degree)).collect();
    // Cubic Hermite blending exactly as ehg128: value and slope pairs at
    // the leaf's corner vertices, the derivative terms scaled by the
    // cell width.
    let mut fitted = vec![0.0f64; n];
    for i in 0..n {
        let z = xs[i];
        let mut leaf = leaves[0];
        for &(llo, lhi) in &leaves {
            if z >= llo - 1e-12 && z <= lhi + 1e-12 {
                leaf = (llo, lhi);
                break;
            }
        }
        let (llo, lhi) = leaf;
        let h = if lhi > llo { (z - llo) / (lhi - llo) } else { 0.0 };
        let vi_lo = vertices.iter().position(|&v| (v - llo).abs() < 1e-9).unwrap_or(0);
        let vi_hi = vertices.iter().position(|&v| (v - lhi).abs() < 1e-9).unwrap_or(vertices.len() - 1);
        let (v0, s0) = fits[vi_lo];
        let (v1, s1) = fits[vi_hi];
        let width = lhi - llo;
        let phi0 = (1.0 - h) * (1.0 - h) * (1.0 + 2.0 * h);
        let phi1 = h * h * (3.0 - 2.0 * h);
        let psi0 = h * (1.0 - h) * (1.0 - h);
        let psi1 = h * h * (h - 1.0);
        fitted[i] = phi0 * v0 + phi1 * v1 + (psi0 * s0 + psi1 * s1) * width;
    }
    fitted
}

/// loess(formula) — the formula's response and predictor symbols resolve
/// in the calling environment exactly as the smoke uses them; the fit
/// object carries class "loess" with GNU's component layout subset:
/// fitted values, residuals, n, and the call exactly as predict.loess
/// and print.loess consume them.
pub fn loess_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands_raw = raw_args(heap, args);
    if operands_raw.is_empty() {
        return Err(EvalError::TypeError("loess requires a formula").into());
    }
    let formula = eval(heap, interner, operands_raw[0], caller_env)?;
    let formula = force_if_promise(heap, interner, formula)?;
    // Parse the formula exactly as y ~ x: the tilde-headed language object
    // with the response as car of cdr and predictor as car of cddr.
    let (mut response, mut predictor) = (ValueId::NIL, ValueId::NIL);
    if let Some(RObject::Language { car: _, cdr, .. }) = heap.object(formula) {
        let mut cursor = *cdr;
        if let Some(RObject::Pairlist { car: c1, cdr: d1, .. }) = heap.object(cursor) {
            response = *c1;
            cursor = *d1;
            if let Some(RObject::Pairlist { car: c2, .. }) = heap.object(cursor) {
                predictor = *c2;
            }
        }
    }
    if response == ValueId::NIL || predictor == ValueId::NIL {
        return Err(EvalError::TypeError("loess requires a y ~ x formula").into());
    }
    let ys_val = eval(heap, interner, response, caller_env)?;
    let ys_val = force_if_promise(heap, interner, ys_val)?;
    let xs_val = eval(heap, interner, predictor, caller_env)?;
    let xs_val = force_if_promise(heap, interner, xs_val)?;
    let ys = reals_of_value(heap, ys_val).map_err(RSignal::from)?;
    let xs = reals_of_value(heap, xs_val).map_err(RSignal::from)?;
    let (xs, ys) = (xs, ys);
    if xs.len() != ys.len() || xs.is_empty() {
        return Err(EvalError::Condition("loess requires equal-length numeric x and y".to_string()).into());
    }
    // span and degree exactly as GNU's defaults; tagged overrides read
    // through the argument tags.
    let mut span = 0.75f64;
    let mut degree = 2usize;
    let mut cursor = args;
    for _ in 0..16 {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        if let Some(RObject::Symbol { name, .. }) = heap.object(tag) {
            let tname = interner.symbol_name(*name).to_string();
            let v = eval(heap, interner, car, caller_env)?;
            match tname.as_str() {
                "span" => {
                    if let Ok(r) = first_real(heap, v) {
                        span = r;
                    }
                }
                "degree" => {
                    if let Ok(r) = first_real(heap, v) {
                        degree = (r as usize).min(2);
                    }
                }
                _ => {}
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    // weights and control exactly as GNU's simpleLoess arranges: the case
    // weights multiply the tricube inside the fit, family selects the
    // robustifying iterations, control carries the surface/statistics/cell
    // parameters through the tagged list.
    let mut weights = vec![1.0f64; xs.len()];
    let mut family = String::from("gaussian");
    let mut surface = String::from("interpolate");
    let mut statistics = String::from("approximate");
    let mut cell = 0.2f64;
    let mut iterations_ctl = 4usize;
    let mut cursor2 = args;
    for _ in 0..16 {
        let (car, cdr, tag) = match heap.object(cursor2) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        if let Some(RObject::Symbol { name, .. }) = heap.object(tag) {
            let tname = interner.symbol_name(*name).to_string();
            let v = eval(heap, interner, car, caller_env)?;
            let v = force_if_promise(heap, interner, v)?;
            match tname.as_str() {
                "weights" => {
                    if let Ok(ws) = reals_of_value(heap, v) {
                        if ws.len() == xs.len() {
                            weights = ws;
                        }
                    }
                }
                "family" => {
                    if let Some(RObject::String(ss)) = heap.object(v) {
                        if let Some(f0) = ss.first() {
                            family = interner.string_text(*f0).to_string();
                        }
                    }
                }
                "control" => {
                    if let Some(RObject::List(items)) = heap.object(v) {
                        let items = items.clone();
                        // read the tagged control entries by name
                        let names = get_attrib(heap, interner, v, "names");
                        if let Some(nm) = names {
                            if let Some(RObject::String(nms)) = heap.object(nm) {
                                let nms = nms.clone();
                                for (ix, item) in items.iter().enumerate() {
                                    let key = interner.string_text(nms[ix]).to_string();
                                    match key.as_str() {
                                        "surface" => {
                                            if let Some(RObject::String(ss)) = heap.object(*item) {
                                                if let Some(f0) = ss.first() {
                                                    surface = interner.string_text(*f0).to_string();
                                                }
                                            }
                                        }
                                        "statistics" => {
                                            if let Some(RObject::String(ss)) = heap.object(*item) {
                                                if let Some(f0) = ss.first() {
                                                    statistics = interner.string_text(*f0).to_string();
                                                }
                                            }
                                        }
                                        "cell" => {
                                            if let Ok(r) = first_real(heap, *item) {
                                                cell = r;
                                            }
                                        }
                                        "iterations" => {
                                            if let Ok(r) = first_real(heap, *item) {
                                                iterations_ctl = r as usize;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor2 = cdr;
    }
    // subset — the observation selection exactly as GNU's model-frame
    // arrangement: indices reorder and repeat exactly as given.
    let mut subset_idx: Option<Vec<usize>> = None;
    let mut want_model = false;
    let mut cursor3 = args;
    for _ in 0..16 {
        let (car, cdr, tag) = match heap.object(cursor3) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        if let Some(RObject::Symbol { name, .. }) = heap.object(tag) {
            let tname = interner.symbol_name(*name).to_string();
            let v = eval(heap, interner, car, caller_env)?;
            let v = force_if_promise(heap, interner, v)?;
            match tname.as_str() {
                "subset" => {
                    if let Ok(rs) = reals_of_value(heap, v) {
                        subset_idx = Some(rs.iter().map(|r| *r as usize).collect());
                    }
                }
                "model" => {
                    want_model = logical_scalar(heap, v);
                }
                _ => {}
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor3 = cdr;
    }
    let (mut xs, mut ys, mut weights) = (xs, ys, weights);
    if let Some(idx) = &subset_idx {
        let nx = xs.clone();
        let ny = ys.clone();
        let nw = weights.clone();
        xs = idx.iter().filter_map(|&i| if i >= 1 && i <= nx.len() { Some(nx[i - 1]) } else { None }).collect();
        ys = idx.iter().filter_map(|&i| if i >= 1 && i <= ny.len() { Some(ny[i - 1]) } else { None }).collect();
        weights = idx.iter().filter_map(|&i| if i >= 1 && i <= nw.len() { Some(nw[i - 1]) } else { None }).collect();
    }
    let iterations = if family == "gaussian" { 1 } else { iterations_ctl.max(1) };
    let direct = surface == "direct";
    // The exact fit chain exactly as simpleLoess arranges: the first
    // iteration computes the statistics, robustness iterations reweight
    // through lowesw's bisquare and refit with statistics none.
    let mut robust = vec![1.0f64; xs.len()];
    let effw: Vec<f64> = weights.iter().zip(robust.iter()).map(|(a, b)| a * b).collect();
    let mut state = crate::loessf::loess_fit_exact(&xs, &ys, &effw, span, degree, span * cell);
    let mut fitted = if direct {
        crate::loessf::direct_fit(&xs, &ys, &effw, span, degree, &xs).0
    } else {
        state.fitted.clone()
    };
    // The statistics exactly as the surface/statistics pair arranges: the
    // interpolate/exact path reads the hat matrix; direct/approximate reads
    // the own-point leverages through the coefficient-table approximation.
    let (mut trl, mut delta1, mut delta2) = if direct {
        let effw2: Vec<f64> = weights.iter().zip(robust.iter()).map(|(a, b)| a * b).collect();
        let (_, rows) = crate::loessf::direct_fit(&xs, &ys, &effw2, span, degree, &xs);
        let trl_v: f64 = rows.iter().enumerate().map(|(i, r)| r[i]).sum();
        let tau = match degree { 0 => 1, 1 => 2, _ => 3 };
        let (d1, d2) = crate::loessf::lowesa(trl_v, xs.len(), 1, tau, 0);
        (trl_v, d1, d2)
    } else {
        (state.trl, state.delta1, state.delta2)
    };
    for _j in 1..iterations {
        // lowesw — the bisquare robustness weights exactly as the Fortran:
        // cmad = 6×median|resid| with the even-n averaged pair, weights
        // (1-(r/cmad)²)² inside, zero outside.
        let resid: Vec<f64> = ys.iter().zip(fitted.iter()).map(|(y, f)| y - f).collect();
        robust = crate::loessf::lowesw(&resid);
        let eff: Vec<f64> = weights.iter().zip(robust.iter()).map(|(a, b)| a * b).collect();
        state = crate::loessf::loess_fit_exact(&xs, &ys, &eff, span, degree, span * cell);
        fitted = if direct {
            crate::loessf::direct_fit(&xs, &ys, &eff, span, degree, &xs).0
        } else {
            state.fitted.clone()
        };
    }
    let residuals: Vec<f64> = ys.iter().zip(fitted.iter()).map(|(y, f)| y - f).collect();
    // The scale exactly as simpleLoess arranges: the symmetric family reads
    // the pseudovalue residuals through lowesp's adjustment exactly.
    let sum_squares: f64 = if family == "gaussian" {
        weights.iter().zip(residuals.iter()).map(|(w, r)| w * r * r).sum()
    } else {
        let pseudo = crate::loessf::lowesp(&ys, &fitted, &weights, &robust);
        let zz = if direct {
            crate::loessf::direct_fit(&xs, &pseudo, &weights, span, degree, &xs).0
        } else {
            crate::loessf::loess_fit_exact(&xs, &pseudo, &weights, span, degree, span * cell).fitted
        };
        weights.iter().zip(pseudo.iter().zip(zz.iter())).map(|(w, (p, z))| {
            let r = p - z;
            w * r * r
        }).sum()
    };
    let s_val = (sum_squares / delta1).sqrt();
    let _ = statistics;
    // The object exactly as GNU's list-of-components shape.
    let fitted_id = heap.alloc(RObject::Real(fitted));
    let residuals_id = heap.alloc(RObject::Real(residuals));
    let n_id = heap.alloc(RObject::Real(vec![xs.len() as f64]));
    let trl_id = heap.alloc(RObject::Real(vec![trl]));
    let d1_id = heap.alloc(RObject::Real(vec![delta1]));
    let d2_id = heap.alloc(RObject::Real(vec![delta2]));
    let s_id = heap.alloc(RObject::Real(vec![s_val]));
    // the fit state carried for predict exactly as the kd component arranges
    let xs_id = heap.alloc(RObject::Real(xs.clone()));
    let ys_id = heap.alloc(RObject::Real(ys.clone()));
    let ws_id = heap.alloc(RObject::Real(weights.clone()));
    let rb_id = heap.alloc(RObject::Real(robust.clone()));
    let pars_id = heap.alloc(RObject::Real(vec![span, degree as f64, cell, if family == "gaussian" { 0.0 } else { 1.0 }, if direct { 1.0 } else { 0.0 }]));
    // x — the predictor matrix exactly as GNU's model arrangement: n×1 with
    // the dim attribute answering matrix subscripts.
    let xmat_id = heap.alloc(RObject::Real(xs.clone()));
    let dim_id = heap.alloc(RObject::Integer(vec![xs.len() as i32, 1]));
    let xmat_id = copy_with_attrib(heap, interner, xmat_id, "dim", dim_id).map_err(RSignal::from)?;
    let (components, names): (Vec<ValueId>, Vec<&str>) = if want_model {
        // model — the model frame approximated as the x matrix exactly as the
        // single-predictor arrangement consumes it.
        let m_id = heap.alloc(RObject::Real(xs.clone()));
        let mdim_id = heap.alloc(RObject::Integer(vec![xs.len() as i32, 1]));
        let m_id = copy_with_attrib(heap, interner, m_id, "dim", mdim_id).map_err(RSignal::from)?;
        (
            vec![n_id, fitted_id, residuals_id, trl_id, d1_id, d2_id, s_id, xs_id, ys_id, ws_id, rb_id, pars_id, xmat_id, m_id],
            vec!["n", "fitted", "residuals", "trace.hat", "one.delta", "two.delta", "s", ".x", ".y", ".weights", ".robust", ".pars", "x", "model"],
        )
    } else {
        (
            vec![n_id, fitted_id, residuals_id, trl_id, d1_id, d2_id, s_id, xs_id, ys_id, ws_id, rb_id, pars_id, xmat_id],
            vec!["n", "fitted", "residuals", "trace.hat", "one.delta", "two.delta", "s", ".x", ".y", ".weights", ".robust", ".pars", "x"],
        )
    };
    let components = components;
    let result = heap.alloc(RObject::List(components));
    let names = names
        .iter()
        .map(|t| interner.intern_string(t))
        .collect::<Vec<_>>();
    let names_id = heap.alloc(RObject::String(names));
    let result = copy_with_attrib(heap, interner, result, "names", names_id).map_err(RSignal::from)?;
    let class_id = heap.alloc(RObject::String(vec![interner.intern_string("loess")]));
    copy_with_attrib(heap, interner, result, "class", class_id).map_err(RSignal::from)
}

/// predict(object) — the loess surface exactly as GNU's predict.loess
/// default: no newdata returns the fitted values. The full generic's
/// UseMethod dispatch belongs with the stats R-level layer; this
/// primitive covers the loess contract the graphics smoke consumes.
pub fn predict_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // UseMethod dispatch exactly as GNU arranges: predict.<class> resolves
    // as a closure in the calling frames and applies with the object and the
    // remaining arguments; the loess fallthrough implements the method
    // directly where no override exists.
    let operands_raw = raw_args(heap, args);
    if operands_raw.is_empty() {
        return Err(EvalError::TypeError("predict requires an object").into());
    }
    let object = eval(heap, interner, operands_raw[0], caller_env)?;
    let object = force_if_promise(heap, interner, object)?;
    let class: Option<String> = get_attrib(heap, interner, object, "class").and_then(|id| {
        match heap.object(id) {
            Some(RObject::String(entries)) if !entries.is_empty() => {
                Some(interner.string_text(entries[0]).to_string())
            }
            _ => None,
        }
    });
    if let Some(cls) = &class {
        let method_name = format!("predict.{}", cls);
        let sym = interner.intern_symbol(&method_name);
        if let Some((_, binding)) = r_core::env::lookup(heap, caller_env, sym) {
            let method = force_if_promise(heap, interner, binding.value)?;
            if matches!(heap.object(method), Some(RObject::Closure { .. })) {
                // Apply exactly as UseMethod arranges: the method call with the
                // original argument list reconstructed as a language object.
                let call = heap.alloc(RObject::Language {
                    car: method,
                    cdr: args,
                    tag: ValueId::NIL,
                });
                return eval(heap, interner, call, caller_env);
            }
        }
    }
    if class.as_deref() == Some("loess")
        && let Some(RObject::List(items)) = heap.object(object) {
            let items = items.clone();
            let names: Vec<String> = get_attrib(heap, interner, object, "names").and_then(|id| {
                match heap.object(id) {
                    Some(RObject::String(entries)) => Some(entries.iter().map(|e| interner.string_text(*e).to_string()).collect()),
                    _ => None,
                }
            }).unwrap_or_default();
            let component = |key: &str| -> Option<ValueId> {
                names.iter().position(|n| n == key).and_then(|pos| items.get(pos).copied())
            };
            // Read the tagged arguments: newdata and se exactly as the method
            // contract arranges.
            let mut newdata: Option<Vec<f64>> = None;
            let mut want_se = false;
            let mut cursor = args;
            for _ in 0..16 {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                if let Some(RObject::Symbol { name, .. }) = heap.object(tag) {
                    let tname = interner.symbol_name(*name).to_string();
                    let v = eval(heap, interner, car, caller_env)?;
                    let v = force_if_promise(heap, interner, v)?;
                    match tname.as_str() {
                        "newdata" => {
                            if let Ok(rs) = reals_of_value(heap, v) {
                                newdata = Some(rs);
                            }
                        }
                        "se" => {
                            want_se = logical_scalar(heap, v);
                        }
                        _ => {}
                    }
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            let fitted = component("fitted");
            if newdata.is_none() && !want_se {
                if let Some(f) = fitted {
                    return Ok(f);
                }
            }
            // Recompute the exact fit exactly as the deterministic surface
            // arranges — the object carries every input.
            let xs = component(".x").and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
            let ys = component(".y").and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
            let ws = component(".weights").and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
            let rb = component(".robust").and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
            let pars = component(".pars").and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
            let s_val = component("s").and_then(|v| first_real(heap, v).ok()).unwrap_or(0.0);
            let d1 = component("one.delta").and_then(|v| first_real(heap, v).ok()).unwrap_or(0.0);
            let d2 = component("two.delta").and_then(|v| first_real(heap, v).ok()).unwrap_or(0.0);
            if xs.is_empty() || pars.len() < 4 {
                return Err(EvalError::TypeError("predict: no method for this object").into());
            }
            let (span, degree, cell) = (pars[0], pars[1] as usize, pars[2]);
            let direct_surface = pars.len() > 4 && pars[4] != 0.0;
            let state = crate::loessf::loess_fit_exact_weighted(&xs, &ys, &ws, &rb, span, degree, span * cell);
            let eval_points: Vec<f64> = match &newdata {
                Some(nd) => nd.clone(),
                None => xs.clone(),
            };
            let eff: Vec<f64> = ws.iter().zip(rb.iter()).map(|(a, b)| a * b).collect();
            let (fits, hat_rows): (Vec<f64>, Vec<Vec<f64>>) = if direct_surface {
                crate::loessf::direct_fit(&xs, &ys, &eff, span, degree, &eval_points)
            } else {
                (
                    eval_points.iter().map(|&z| crate::loessf::predict_at(&state, &[z])).collect(),
                    eval_points.iter().map(|&z| crate::loessf::hat_row_at(&state, &[z], xs.len())).collect(),
                )
            };
            if !want_se {
                let fit_id = heap.alloc(RObject::Real(fits));
                return Ok(fit_id);
            }
            // se.fit = s × sqrt(sum_j L(z,j)² / weights_j) exactly as predLoess.
            let ses: Vec<f64> = hat_rows.iter().map(|row| {
                let acc: f64 = row.iter().zip(ws.iter()).map(|(l, w)| l * l / w).sum();
                s_val * acc.sqrt()
            }).collect();
            let fit_id = heap.alloc(RObject::Real(fits));
            let se_id = heap.alloc(RObject::Real(ses));
            let df_id = heap.alloc(RObject::Real(vec![d1 * d1 / d2]));
            let rs_id = heap.alloc(RObject::Real(vec![s_val]));
            let out = heap.alloc(RObject::List(vec![fit_id, se_id, rs_id, df_id]));
            let nm = ["fit", "se.fit", "residual.scale", "df"]
                .iter()
                .map(|t| interner.intern_string(t))
                .collect::<Vec<_>>();
            let nm_id = heap.alloc(RObject::String(nm));
            copy_with_attrib(heap, interner, out, "names", nm_id).map_err(RSignal::from)
        } else {
            Err(EvalError::TypeError("predict: no method for this object").into())
        }
}

/// GNU's RNGkind: query and set the generator, normal and sample kinds.
/// The pure world carries the Mersenne-Twister/Inversion pair as its only
/// generator exactly as the golden streams pin it; kind and normal.kind
/// accept their supported values or NULL exactly as GNU's no-change form,
/// anything else is GNU's unimplemented-generator error. sample.kind
/// switches the walker branch between Rounding and Rejection exactly as
/// random.c. Returns the PRIOR triple invisibly exactly as GNU.
pub fn rngkind_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use r_core::eval::eval;
    let prior_kind = "Mersenne-Twister";
    let prior_normal = "Inversion";
    let prior_sample = if heap.sample_kind_rounding { "Rounding" } else { "Rejection" };
    // Walk the argument pairlist exactly as GNU's dotted-pair matching:
    // tagged arguments route by name, untagged fill positions in order.
    let mut values: [Option<ValueId>; 3] = [None, None, None];
    let mut position = 0usize;
    let mut cursor = args;
    while let Some((car, cdr, tag)) = pairlist_parts(heap, cursor) {
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let slot = match tag_name.as_deref() {
            Some("kind") => Some(0),
            Some("normal.kind") => Some(1),
            Some("sample.kind") => Some(2),
            Some(_) => None,
            None => {
                while position < 3 && values[position].is_some() {
                    position += 1;
                }
                if position < 3 { Some(position) } else { None }
            }
        };
        if let Some(slot) = slot
            && values[slot].is_none() {
                values[slot] = Some(car);
            }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    for (slot, value) in values.iter().enumerate() {
        let Some(raw) = value else { continue };
        let raw = *raw;
        if raw == ValueId::NIL {
            continue;
        }
        let resolved = match eval(heap, interner, raw, caller_env) {
            Ok(v) => force_if_promise(heap, interner, v)?,
            Err(_) => raw,
        };
        let text = match heap.object(resolved) {
            Some(RObject::String(entries)) if !entries.is_empty() => {
                interner.string_text(entries[0]).to_string()
            }
            _ => continue,
        };
        match slot {
            0 => {
                if text != "Mersenne-Twister" {
                    return Err(RSignal::Error(EvalError::Condition(
                        format!("'{text}' is not a valid RNG kind in this build"),
                    )));
                }
            }
            1 => {
                if text != "Inversion" {
                    return Err(RSignal::Error(EvalError::Condition(
                        format!("'{text}' is not a valid normal kind in this build"),
                    )));
                }
            }
            _ => {
                match text.as_str() {
                    "Rounding" => heap.sample_kind_rounding = true,
                    "Rejection" => heap.sample_kind_rounding = false,
                    _ => {
                        return Err(RSignal::Error(EvalError::Condition(
                            format!("'{text}' is not a valid sample kind"),
                        )));
                    }
                }
            }
        }
    }
    let interned = [prior_kind, prior_normal, prior_sample]
        .iter()
        .map(|t| interner.intern_string(t))
        .collect();
    Ok(heap.alloc(RObject::String(interned)))
}

/// walker_ProbSampleReplace exactly as random.c: the alias tables build
/// over q = n*p with the H/L cursors, the draw consuming one rejection
/// index plus one uniform per sample exactly as the rejection branch.
pub fn walker_sample_pure(heap: &mut Heap, p: &mut [f64], size: usize) -> Vec<usize> {
    let n = p.len();
    let mut a = vec![0i32; n];
    let mut hl = vec![0i32; n];
    let mut q = vec![0.0f64; n];
    let mut h = 0usize;
    let mut l = n;
    for i in 0..n {
        q[i] = p[i] * n as f64;
        if q[i] < 1.0 {
            hl[h] = i as i32;
            h += 1;
        } else {
            l -= 1;
            hl[l] = i as i32;
        }
    }
    if h > 0 && l < n {
        for k in 0..n.saturating_sub(1) {
            let i = hl[k] as usize;
            let j = hl[l] as usize;
            a[i] = j as i32;
            q[j] += q[i] - 1.0;
            if q[j] < 1.0 {
                l += 1;
            }
            if l >= n {
                break;
            }
        }
    }
    for (i, qi) in q.iter_mut().enumerate() {
        *qi += i as f64;
    }
    let mut out = Vec::with_capacity(size);
    for _ in 0..size {
        // GNU's sample.kind branch exactly: Rounding draws one uniform and
        // reuses its fractional part as the coin flip; Rejection draws the
        // index through the bit-assembly rejection path plus a second
        // uniform for the comparison.
        let (k, r_u) = if heap.sample_kind_rounding {
            let r_u = heap.rng.unif() * n as f64;
            (r_u as usize, r_u)
        } else {
            let k = heap.rng.unif_index_rejection(n as f64) as usize;
            (k, k as f64 + heap.rng.unif())
        };
        out.push(if r_u < q[k] { k } else { a[k].max(0) as usize });
    }
    out
}

pub fn sample_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    int_form: bool,
) -> EvalResult {
    // GNU's formals exactly: x/n, size, replace, prob — named arguments
    // bind by name and positionals fill the gaps exactly as pmatch.
    let formal_names: &[&str] = if int_form {
        &["n", "size", "replace", "prob"]
    } else {
        &["x", "size", "replace", "prob"]
    };
    let slots = eval_matched_args(heap, interner, args, caller_env, formal_names)?;
    let mut positional: Vec<ValueId> = Vec::new();
    for slot in slots.iter().take(4) {
        match slot {
            Some(v) => positional.push(*v),
            None => break,
        }
    }
    if positional.is_empty() {
        return Ok(heap.alloc(RObject::Integer(Vec::new())));
    }
    // sample.int(n, size, replace): the population is 1:n.
    let (population, _pop_names): (Vec<i64>, Vec<String>) = if int_form {
        let n = match heap.object(positional[0]) {
            Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.first().copied().unwrap_or(0) as i64,
            Some(RObject::Real(vals)) => vals.first().copied().unwrap_or(0.0) as i64,
            _ => 0,
        };
        ((1..=n.max(0)).collect(), Vec::new())
    } else {
        // sample(x, ...): a bare positive number means the population 1:x
        // exactly as GNU's numeric shortcut.
        match heap.object(positional[0]) {
            Some(RObject::Real(vals)) if vals.len() == 1 && vals[0] >= 1.0 => {
                ((1..=vals[0] as i64).collect(), Vec::new())
            }
            Some(RObject::Integer(vals)) if vals.len() == 1 && vals[0] >= 1 => {
                ((1..=vals[0] as i64).collect(), Vec::new())
            }
            _ => (Vec::new(), Vec::new()),
        }
    };
    let size = slots[1].and_then(|v| match heap.object(v) {
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.first().copied().map(|x| x as usize),
        Some(RObject::Real(vals)) => vals.first().copied().map(|x| x as usize),
        _ => None,
    });
    let replace = slots[2].map(|v| matches!(heap.object(v), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) == 1)).unwrap_or(false);
    let probs: Vec<f64> = slots[3].and_then(|v| match heap.object(v) {
        Some(RObject::Real(vals)) => Some(vals.clone()),
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => Some(vals.iter().map(|x| *x as f64).collect()),
        _ => None,
    }).unwrap_or_default();

    if !population.is_empty() {
        // Integer population path — sample.int and the numeric shortcut.
        let n = population.len();
        let k = size.unwrap_or(n);
        if !replace && k > n {
            return Err(RSignal::Error(EvalError::Condition(
                "cannot take a sample larger than the population when 'replace = FALSE'".to_string(),
            )));
        }
        // GNU's uniform branch exactly as random.c do_sample: independent
        // full-population draws when replacing (or a single draw), else
        // the Fisher-Yates copy-out loop pool[j] = pool[--remaining].
        if !probs.is_empty() {
            if probs.len() != n {
                return Err(RSignal::Error(EvalError::Condition(
                    "incorrect number of probabilities".to_string(),
                )));
            }
            let indices = weighted_sample_indices_pure(heap, probs.as_slice(), k, replace)?;
            return Ok(heap.alloc(RObject::Integer(
                indices.iter().map(|i| population[*i] as i32).collect(),
            )));
        }
        let mut chosen: Vec<i64> = Vec::with_capacity(k);
        if replace || k < 2 {
            for _ in 0..k {
                let draw = heap.rng.unif_index_rejection(n as f64) as usize;
                chosen.push(population[draw.min(n - 1)]);
            }
        } else {
            let mut pool: Vec<usize> = (0..n).collect();
            let mut remaining = n;
            for _ in 0..k {
                let j = heap.rng.unif_index_rejection(remaining as f64) as usize;
                let j = j.min(remaining - 1);
                chosen.push(population[pool[j]]);
                remaining -= 1;
                pool[j] = pool[remaining];
            }
        }
        return Ok(heap.alloc(RObject::Integer(chosen.into_iter().map(|v| v as i32).collect())));
    }

    // General vector population — preserve type and names exactly.
    let source = positional[0];
    let len = vector_length(heap, source).unwrap_or(0);
    let k = size.unwrap_or(len);
    let names: Vec<String> = match get_attrib(heap, interner, source, "names") {
        Some(names_id) => string_vec(heap, interner, names_id),
        None => Vec::new(),
    };
    if !replace && k > len {
        return Err(RSignal::Error(EvalError::Condition(
            "cannot take a sample larger than the population when 'replace = FALSE'".to_string(),
        )));
    }
    let mut chosen_idx: Vec<usize> = Vec::with_capacity(k);
    if !probs.is_empty() {
        if probs.len() != len {
            return Err(RSignal::Error(EvalError::Condition(
                "incorrect number of probabilities".to_string(),
            )));
        }
        chosen_idx = weighted_sample_indices_pure(heap, probs.as_slice(), k, replace)?;
    } else if replace || k < 2 {
        for _ in 0..k {
            let draw = heap.rng.unif_index_rejection(len as f64) as usize;
            chosen_idx.push(draw.min(len - 1));
        }
    } else {
        let mut pool: Vec<usize> = (0..len).collect();
        let mut remaining = len;
        for _ in 0..k {
            let j = heap.rng.unif_index_rejection(remaining as f64) as usize;
            let j = j.min(remaining - 1);
            chosen_idx.push(pool[j]);
            remaining -= 1;
            pool[j] = pool[remaining];
        }
    }
    let out_names: Vec<String> = if names.is_empty() {
        Vec::new()
    } else {
        chosen_idx.iter().map(|i| names.get(*i).cloned().unwrap_or_default()).collect()
    };
    let result = match heap.object(source).cloned() {
        Some(RObject::Real(vals)) => heap.alloc(RObject::Real(chosen_idx.iter().filter_map(|i| vals.get(*i).copied()).collect())),
        Some(RObject::Integer(vals)) => heap.alloc(RObject::Integer(chosen_idx.iter().filter_map(|i| vals.get(*i).copied()).collect())),
        Some(RObject::Logical(vals)) => heap.alloc(RObject::Logical(chosen_idx.iter().filter_map(|i| vals.get(*i).copied()).collect())),
        Some(RObject::String(vals)) => heap.alloc(RObject::String(chosen_idx.iter().filter_map(|i| vals.get(*i).copied()).collect())),
        _ => heap.alloc(RObject::Integer(Vec::new())),
    };
    if !out_names.is_empty() {
        let names_obj = heap.alloc(RObject::String(out_names.iter().map(|n| interner.intern_string(n)).collect()));
        return copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from);
    }
    Ok(result)
}

/// set.seed(x) — GNU's seeding entry exactly: integer truncation of
/// the supplied value seeds the Mersenne stream; the return is NULL
/// invisibly exactly as RNGkind's documented shape.
pub fn set_seed_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x"])?;
    let seed = slots[0]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or(0.0);
    heap.rng.set_seed(seed as i64);
    heap.visible = false;
    Ok(ValueId::NIL)
}

/// runif(n, min, max) — n draws scaled onto [min, max] exactly as
/// GNU's stats layer composes over unif_rand.
pub fn runif_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["n", "min", "max"])?;
    let n = slots[0]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or(1.0)
        .max(0.0) as usize;
    let lo = slots[1]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or(0.0);
    let hi = slots[2]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or(1.0);
    let span = hi - lo;
    let mut out = Vec::with_capacity(n.min(10_000_000));
    for _ in 0..n.min(10_000_000) {
        let u = heap.rng.unif();
        out.push(lo + span * u);
    }
    Ok(heap.alloc(RObject::Real(out)))
}

/// rnorm(n, mean, sd) — inversion normals scaled exactly as GNU.
pub fn rnorm_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["n", "mean", "sd"])?;
    let n = slots[0]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or(1.0)
        .max(0.0) as usize;
    let mean = slots[1]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or(0.0);
    let sd = slots[2]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or(1.0);
    let mut out = Vec::with_capacity(n.min(10_000_000));
    for _ in 0..n.min(10_000_000) {
        let z = heap.rng.norm();
        out.push(mean + sd * z);
    }
    Ok(heap.alloc(RObject::Real(out)))
}

/// Extract finite pairs from two equal-length numeric vectors — the
/// complete-case discipline cor and cov share exactly as GNU's
/// default use="everything" treats NA by propagation.
pub fn finite_pairs(xs: &[f64], ys: &[f64]) -> Vec<(f64, f64)> {
    xs.iter()
        .copied()
        .zip(ys.iter().copied())
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .collect()
}

/// cov(x, y) — GNU's sample covariance exactly: centered cross
/// products over n-1, with cov(x) the variance exactly as var.
pub fn cov_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "y"])?;
    let xs = slots[0].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let ys = match slots[1].and_then(|v| reals_of_value(heap, v).ok()) {
        Some(v) => v,
        None => xs.clone(),
    };
    if xs.len() < 2 || ys.len() != xs.len() {
        return Ok(heap.alloc(RObject::Real(vec![f64::NAN])));
    }
    let pairs = finite_pairs(&xs, &ys);
    let n = pairs.len() as f64;
    if pairs.len() < 2 {
        return Ok(heap.alloc(RObject::Real(vec![f64::NAN])));
    }
    let mx = pairs.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pairs.iter().map(|p| p.1).sum::<f64>() / n;
    let cross: f64 = pairs.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    Ok(heap.alloc(RObject::Real(vec![cross / (n - 1.0)])))
}

/// cor(x, y) — Pearson exactly as GNU: covariance over the product
/// of standard deviations computed on the same complete pairs.
pub fn cor_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "y"])?;
    let xs = slots[0].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let ys = match slots[1].and_then(|v| reals_of_value(heap, v).ok()) {
        Some(v) => v,
        None => xs.clone(),
    };
    let pairs = finite_pairs(&xs, &ys);
    let n = pairs.len() as f64;
    if pairs.len() < 2 {
        return Ok(heap.alloc(RObject::Real(vec![f64::NAN])));
    }
    let mx = pairs.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pairs.iter().map(|p| p.1).sum::<f64>() / n;
    let cross: f64 = pairs.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let vx: f64 = pairs.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
    let vy: f64 = pairs.iter().map(|p| (p.1 - my) * (p.1 - my)).sum();
    let denom = (vx * vy).sqrt();
    let answer = if denom == 0.0 { f64::NAN } else { cross / denom };
    Ok(heap.alloc(RObject::Real(vec![answer])))
}

/// quantile(x, probs) — GNU's type 7 exactly as the default: linear
/// interpolation at h = 1 + (n-1)p, type 7 being the continuous
/// mode of Hyndman and Fan's taxonomy.
pub fn quantile_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "probs"])?;
    let mut xs: Vec<f64> = slots[0]
        .and_then(|v| reals_of_value(heap, v).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|v| v.is_finite())
        .collect();
    if xs.is_empty() {
        return Ok(heap.alloc(RObject::Real(Vec::new())));
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let probs = match slots[1].and_then(|v| reals_of_value(heap, v).ok()) {
        Some(v) if !v.is_empty() => v,
        _ => vec![0.0, 0.25, 0.5, 0.75, 1.0],
    };
    let n = xs.len() as f64;
    let out: Vec<f64> = probs
        .iter()
        .map(|p| {
            let h = 1.0 + (n - 1.0) * p;
            let lo = h.floor().max(1.0).min(n) as usize;
            let hi = (lo + 1).min(n as usize);
            let frac = h - lo as f64;
            let xlo = xs[lo - 1];
            let xhi = xs[hi - 1];
            xlo + frac * (xhi - xlo)
        })
        .collect();
    let result = heap.alloc(RObject::Real(out));
    // GNU names every quantile with its percent — integer percents print
    // bare exactly as format(100*p) trims trailing zeros.
    let names: Vec<_> = probs
        .iter()
        .map(|p| {
            let pct = 100.0 * p;
            let text = if pct.fract() == 0.0 && pct.abs() < 1e15 {
                format!("{}%", pct as i64)
            } else {
                let mut t = format!("{:.7}", pct);
                while t.contains('.') && t.ends_with('0') {
                    t.pop();
                }
                if t.ends_with('.') {
                    t.pop();
                }
                format!("{}%", t)
            };
            interner.intern_string(&text)
        })
        .collect();
    let names_val = heap.alloc(RObject::String(names));
    copy_with_attrib(heap, interner, result, "names", names_val).map_err(RSignal::Error)
}

/// mad(x, center, constant) — GNU's median absolute deviation exactly:
/// constant times the median of absolute deviations from center,
/// center defaulting to the median itself.
pub fn mad_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "center", "constant"])?;
    let xs: Vec<f64> = slots[0]
        .and_then(|v| reals_of_value(heap, v).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|v| v.is_finite())
        .collect();
    if xs.is_empty() {
        return Ok(heap.alloc(RObject::Real(vec![f64::NAN])));
    }
    let median_of = |values: &mut Vec<f64>| -> f64 {
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = values.len();
        if n % 2 == 1 {
            values[n / 2]
        } else {
            (values[n / 2 - 1] + values[n / 2]) / 2.0
        }
    };
    let center = slots[1]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or_else(|| median_of(&mut xs.clone()));
    let constant = slots[2]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or(1.4826);
    let mut devs: Vec<f64> = xs.iter().map(|x| (x - center).abs()).collect();
    let mad = median_of(&mut devs);
    Ok(heap.alloc(RObject::Real(vec![constant * mad])))
}

/// weighted.mean(x, w) — GNU's weighted average exactly: weights
/// recycle against x, zero total weight answers NaN.
pub fn weighted_mean_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "w"])?;
    let xs = slots[0].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    if xs.is_empty() {
        return Ok(heap.alloc(RObject::Real(vec![f64::NAN])));
    }
    let ws = match slots[1].and_then(|v| reals_of_value(heap, v).ok()) {
        Some(v) if !v.is_empty() => v,
        _ => vec![1.0; xs.len()],
    };
    let mut total = 0.0;
    let mut weight = 0.0;
    for i in 0..xs.len() {
        let w = ws[i % ws.len()];
        if xs[i].is_finite() && w.is_finite() {
            total += xs[i] * w;
            weight += w;
        }
    }
    let answer = if weight == 0.0 { f64::NAN } else { total / weight };
    Ok(heap.alloc(RObject::Real(vec![answer])))
}

/// The shared vectorized random-generation skeleton: evaluate n plus the
/// distribution's parameter vectors, install the heap stream as nmath's
/// uniform source, then draw with GNU's parameter recycling exactly —
/// parameter i of draw k is element k % len exactly as R's vectorized
/// random functions recycle.
pub fn r_zoo(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    param_names: &[&str],
    defaults: &[f64],
    draw: impl Fn(&mut dyn FnMut() -> f64, &mut r_math::state::MathState, &[f64]) -> f64,
) -> EvalResult {
    let mut names: Vec<&str> = vec!["n"];
    names.extend_from_slice(param_names);
    let slots = eval_matched_args(heap, interner, args, caller_env, &names)?;
    let n = slots[0]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or(1.0)
        .max(0.0) as usize;
    let mut params: Vec<Vec<f64>> = Vec::with_capacity(param_names.len());
    for (index, _name) in param_names.iter().enumerate() {
        let values = slots[index + 1]
            .and_then(|v| reals_of_value(heap, v).ok())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| vec![defaults[index]]);
        params.push(values);
    }
    let mut out = Vec::with_capacity(n.min(10_000_000));
    {
        let rng = &mut heap.rng;
        let mut unif = move || rng.unif();
        for i in 0..n.min(10_000_000) {
            let draw_params: Vec<f64> = params.iter().map(|v| v[i % v.len()]).collect();
            out.push(draw(&mut unif, &mut heap.math, &draw_params));
        }
    }
    Ok(heap.alloc(RObject::Real(out)))
}

macro_rules! zoo_primitive {
    ($name:ident, [$($param:literal),+], [$($default:expr),+], $draw:expr) => {
        pub fn $name(
            heap: &mut Heap,
            interner: &mut SymbolInterner,
            args: ValueId,
            caller_env: ValueId,
        ) -> EvalResult {
            r_zoo(heap, interner, args, caller_env, &[$($param),+], &[$($default),+], $draw)
        }
    };
}

zoo_primitive!(rbinom_primitive, ["size", "prob"], [1.0, 0.5], |rng: &mut dyn FnMut() -> f64, ms: &mut r_math::state::MathState, ps: &[f64]| r_math::dist::binomial::rbinom(rng, ms, ps[0], ps[1]));
zoo_primitive!(rpois_primitive, ["lambda"], [1.0], |rng: &mut dyn FnMut() -> f64, ms: &mut r_math::state::MathState, ps: &[f64]| r_math::dist::poisson::rpois(rng, ms, ps[0]));
zoo_primitive!(rbeta_primitive, ["shape1", "shape2"], [1.0, 1.0], |rng: &mut dyn FnMut() -> f64, ms: &mut r_math::state::MathState, ps: &[f64]| r_math::dist::beta::rbeta(rng, ms, ps[0], ps[1]));
zoo_primitive!(rchisq_primitive, ["df"], [1.0], |rng: &mut dyn FnMut() -> f64, ms: &mut r_math::state::MathState, ps: &[f64]| r_math::dist::chisq::rchisq(rng, ms, ps[0]));
zoo_primitive!(rt_primitive, ["df"], [1.0], |rng: &mut dyn FnMut() -> f64, ms: &mut r_math::state::MathState, ps: &[f64]| r_math::dist::t_dist::rt(rng, ms, ps[0]));
zoo_primitive!(rf_primitive, ["df1", "df2"], [1.0, 1.0], |rng: &mut dyn FnMut() -> f64, ms: &mut r_math::state::MathState, ps: &[f64]| r_math::dist::f_dist::rf(rng, ms, ps[0], ps[1]));
zoo_primitive!(rlnorm_primitive, ["meanlog", "sdlog"], [0.0, 1.0], |rng: &mut dyn FnMut() -> f64, _ms: &mut r_math::state::MathState, ps: &[f64]| r_math::dist::lnorm::rlnorm(rng, ps[0], ps[1]));
zoo_primitive!(rlogis_primitive, ["location", "scale"], [0.0, 1.0], |rng: &mut dyn FnMut() -> f64, _ms: &mut r_math::state::MathState, ps: &[f64]| r_math::dist::logistic::rlogis(rng, ps[0], ps[1]));
zoo_primitive!(rcauchy_primitive, ["location", "scale"], [0.0, 1.0], |rng: &mut dyn FnMut() -> f64, _ms: &mut r_math::state::MathState, ps: &[f64]| r_math::dist::cauchy::rcauchy(rng, ps[0], ps[1]));
zoo_primitive!(rgeom_primitive, ["prob"], [0.5], |rng: &mut dyn FnMut() -> f64, ms: &mut r_math::state::MathState, ps: &[f64]| r_math::dist::geometric::rgeom(rng, ms, ps[0]));
zoo_primitive!(rweibull_primitive, ["shape", "scale"], [1.0, 1.0], |rng: &mut dyn FnMut() -> f64, _ms: &mut r_math::state::MathState, ps: &[f64]| r_math::dist::weibull::rweibull(rng, ps[0], ps[1]));
zoo_primitive!(rnbinom_primitive, ["size", "prob"], [1.0, 0.5], |rng: &mut dyn FnMut() -> f64, ms: &mut r_math::state::MathState, ps: &[f64]| r_math::dist::nbinom::rnbinom(rng, ms, ps[0], ps[1]));

/// rgamma(n, shape, rate, scale) — GNU's parameterization exactly: scale
/// defaults to 1/rate and an explicit scale overrides rate.
pub fn rgamma_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["n", "shape", "rate", "scale"])?;
    let n = slots[0]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or(1.0)
        .max(0.0) as usize;
    let shapes = slots[1]
        .and_then(|v| reals_of_value(heap, v).ok())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec![1.0]);
    let rates = slots[2]
        .and_then(|v| reals_of_value(heap, v).ok())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec![1.0]);
    let scales = slots[3].and_then(|v| reals_of_value(heap, v).ok()).filter(|v| !v.is_empty());
    let mut out = Vec::with_capacity(n.min(10_000_000));
    {
        let rng = &mut heap.rng;
        let mut unif = move || rng.unif();
        for i in 0..n.min(10_000_000) {
            let shape = shapes[i % shapes.len()];
            let scale = match &scales {
                Some(values) => values[i % values.len()],
                None => 1.0 / rates[i % rates.len()],
            };
            out.push(r_math::dist::gamma::rgamma(&mut unif, &mut heap.math, shape, scale));
        }
    }
    Ok(heap.alloc(RObject::Real(out)))
}

/// rexp(n, rate) — GNU's rate parameterization exactly: the kernel takes
/// the scale, which is the rate's reciprocal.
pub fn rexp_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    r_zoo(heap, interner, args, caller_env, &["rate"], &[1.0], |rng: &mut dyn FnMut() -> f64, _ms: &mut r_math::state::MathState, ps: &[f64]| {
        r_math::dist::exponential::rexp(rng, 1.0 / ps[0])
    })
}

/// rhyper(nn, m, n, k) — GNU's urn parameterization exactly: m white
/// balls, n black, k drawn.
pub fn rhyper_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["nn", "m", "n", "k"])?;
    let nn = slots[0]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or(1.0)
        .max(0.0) as usize;
    let ms = slots[1].and_then(|v| reals_of_value(heap, v).ok()).filter(|v| !v.is_empty()).unwrap_or_else(|| vec![1.0]);
    let ns = slots[2].and_then(|v| reals_of_value(heap, v).ok()).filter(|v| !v.is_empty()).unwrap_or_else(|| vec![1.0]);
    let ks = slots[3].and_then(|v| reals_of_value(heap, v).ok()).filter(|v| !v.is_empty()).unwrap_or_else(|| vec![1.0]);
    let mut out = Vec::with_capacity(nn.min(10_000_000));
    {
        let rng = &mut heap.rng;
        let mut unif = move || rng.unif();
        for i in 0..nn.min(10_000_000) {
            out.push(r_math::dist::hypergeometric::rhyper(
                &mut unif,
                &mut heap.math,
                ms[i % ms.len()],
                ns[i % ns.len()],
                ks[i % ks.len()],
            ));
        }
    }
    Ok(heap.alloc(RObject::Real(out)))
}
