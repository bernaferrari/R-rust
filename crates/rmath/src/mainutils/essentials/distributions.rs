// Distribution-function builtins: d*/p*/q* wrappers (dnorm..dmultinom) and
// their shared vectorization helpers (dpq_* family, dist_match). Extracted
// from essentials.rs as the first step of incremental decomposition
// (rport-btb7). Re-exported by essentials.rs so the builtin registration
// table paths (crate::mainutils::essentials::do_dnorm etc.) are unchanged.
// Shared helpers (real_or_default, elt_real_safe, logical_arg) remain in
// essentials.rs and are reached via super::.
//
// Vectorization mirrors stock distn.c (SETUP_MathN / FINISH_MathN) plus the
// stats R wrapper defaults: every numeric formal participates in recycling,
// the result length is the max operand length, any zero-length operand gives
// a zero-length result, and result attributes come from the operand whose
// length matches.
#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
use super::*;

// ---------------------------------------------------------------------------
// Argument matching
// ---------------------------------------------------------------------------

/// Match supplied args against a formals-name table the way stock R's
/// `matchArgs_NR` does for closures without `...`:
/// 1. exact tag matches (duplicates error),
/// 2. partial tag matches (erroring on ambiguity),
/// 3. untagged args fill remaining slots positionally.
///
/// Returns the per-formal slot values plus `filled` flags, because stock R
/// distinguishes "formal not supplied" (default applies) from "supplied
/// NULL" (fails the C-level isNumeric check). A positional gap keeps
/// `R_MissingArg` in its slot (stock raises the missing error when the
/// formal has no default, and falls back to the default otherwise); a
/// tagged empty argument (`f(x=)`) is dropped entirely. A supplied arg
/// that matches no formal is an "unused argument" error, mirroring stock R
/// (e.g. `pnorm(1, foo=2)` errors).
unsafe fn dist_match(args: SEXP, names: &[&str]) -> (Vec<SEXP>, Vec<bool>) {
    unsafe {
        let mut out: Vec<SEXP> = names.iter().map(|_| R_NilValue()).collect();
        let mut filled = vec![false; names.len()];
        let mut supplied: Vec<(Option<String>, SEXP)> = Vec::new();
        let mut cur = args;
        while !cur.is_null() && cur != R_NilValue() {
            let value = CAR(cur);
            let tagged = tag_name(cur);
            if value == crate::sexp::globals::R_MissingArg() && tagged.is_some() {
                // `f(x=)`: dropped, the formal keeps its default.
                cur = CDR(cur);
                continue;
            }
            supplied.push((tagged, value));
            cur = CDR(cur);
        }

        // Pass 1: exact tag matches.
        let mut matched = vec![false; supplied.len()];
        for i in 0..supplied.len() {
            if let Some(tag) = supplied[i].0.as_deref() {
                if let Some(j) = names.iter().position(|n| *n == tag) {
                    if filled[j] {
                        base_error(format!(
                            "formal argument \"{tag}\" matched by multiple actual arguments"
                        ));
                    }
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
            let deparsed = deparse_arg_value(supplied[i].1);
            if let Some(tag) = supplied[i].0.as_deref() {
                base_error(format!("unused argument ({tag} = {deparsed})"));
            }
            while slot < names.len() && filled[slot] {
                slot += 1;
            }
            if slot >= names.len() {
                base_error(format!("unused argument ({deparsed})"));
            }
            out[slot] = supplied[i].1;
            filled[slot] = true;
            matched[i] = true;
        }

        (out, filled)
    }
}

/// Deparse an argument value for stock-style "unused argument (...)"
/// messages (stock uses the deparsed value, not a placeholder).
unsafe fn deparse_arg_value(value: SEXP) -> String {
    unsafe {
        let text = crate::mainutils::deparse::deparse1line(value, false);
        if !text.is_null() && text != R_NilValue() && XLENGTH(text) > 0 {
            let chars = crate::sexp::accessors::CHAR(crate::sexp::accessors::STRING_ELT(text, 0));
            if !chars.is_null() {
                return std::ffi::CStr::from_ptr(chars)
                    .to_string_lossy()
                    .into_owned();
            }
        }
        String::new()
    }
}

/// A supplied-and-not-missing slot (explicit NULL counts as supplied).
fn dpq_supplied(slots: &[SEXP], filled: &[bool], idx: usize) -> bool {
    unsafe { filled[idx] && slots[idx] != crate::sexp::globals::R_MissingArg() }
}

// ---------------------------------------------------------------------------
// dpq vectorization (stock SETUP_MathN / FINISH_MathN semantics)
// ---------------------------------------------------------------------------

/// Stock distn.c `R_MSG_NONNUM_MATH` (capital N) used by the d/p/q C level.
const DPQ_NONNUM: &str = "Non-numeric argument to mathematical function";

/// One vectorized operand of a d/p/q builtin.
enum DpqOperand {
    /// Formal fell back to its scalar default (not supplied, or supplied as
    /// a positional gap on a defaulted formal).
    Default(f64),
    /// Supplied numeric (logical/integer/real) vector; recycles mod length.
    Numeric(SEXP),
    /// Supplied numeric vector contributing `1/x` element-wise — the
    /// `scale = 1/rate` formals whose conversion happens at R level as a
    /// binary operation.
    Inverse(SEXP),
}
fn dpq_operand_len(op: &DpqOperand) -> crate::sexp::ffi::R_xlen_t {
    match op {
        DpqOperand::Default(_) => 1,
        DpqOperand::Numeric(s) | DpqOperand::Inverse(s) => unsafe { XLENGTH(*s) },
    }
}

/// Stock `argument "x" is missing, with no default`.
fn dpq_missing(name: &str) -> ! {
    base_error(format!("argument \"{name}\" is missing, with no default"))
}

/// Resolve one numeric dpq formal.
///
/// `default` is the formal's R-level default (`None` for required formals).
/// A missing/gap slot falls back to the default (required formals raise the
/// missing error); an explicit NULL or a non-numeric value fails stock's
/// `isNumeric` check with `nonnum_error` (the rate formals convert at R
/// level and use the binary-operator message instead).
unsafe fn dpq_num(
    slots: &[SEXP],
    filled: &[bool],
    names: &[&str],
    idx: usize,
    default: Option<f64>,
    nonnum_error: &str,
) -> DpqOperand {
    unsafe {
        let slot = slots[idx];
        if !dpq_supplied(slots, filled, idx) {
            return match default {
                Some(d) => DpqOperand::Default(d),
                None => dpq_missing(names[idx]),
            };
        }
        if slot != R_NilValue() {
            let t = TYPEOF(slot);
            if t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                return DpqOperand::Numeric(slot);
            }
        }
        base_error(nonnum_error)
    }
}

/// Resolve a `scale = 1/rate` style formal. Returns `None` when the formal
/// was not supplied (the caller applies the scalar default). An explicit
/// NULL still contributes (R computes `1/NULL` = `numeric(0)`), and a
/// non-numeric value fails with the R-level binary-operator message.
unsafe fn dpq_inverse(slots: &[SEXP], filled: &[bool], idx: usize) -> Option<DpqOperand> {
    unsafe {
        let slot = slots[idx];
        if !dpq_supplied(slots, filled, idx) {
            return None;
        }
        if slot == R_NilValue() {
            return Some(DpqOperand::Inverse(slot));
        }
        let t = TYPEOF(slot);
        if t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            Some(DpqOperand::Inverse(slot))
        } else {
            base_error("non-numeric argument to binary operator")
        }
    }
}

/// Resolve a flag formal (`log`, `lower.tail`, `log.p`) the way stock's
/// `asInteger()` does: first element, real values truncate, `NA` counts as
/// nonzero (true).
unsafe fn dpq_flag(slots: &[SEXP], filled: &[bool], idx: usize, default: bool) -> bool {
    unsafe {
        let slot = slots[idx];
        if !dpq_supplied(slots, filled, idx) || slot == R_NilValue() {
            return default;
        }
        // A zero-length flag coerces to NA_INTEGER, which is nonzero and
        // therefore true (stock `asInteger()`).
        if XLENGTH(slot) == 0 {
            return true;
        }
        let t = TYPEOF(slot);
        if t == SEXPTYPE::REALSXP {
            (*REAL(slot)).trunc() != 0.0
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            *INTEGER(slot) != 0
        } else {
            default
        }
    }
}

/// SHALLOW_DUPLICATE_ATTRIB: copy every attribute of `src` onto `dst`.
unsafe fn dpq_copy_attrib(dst: SEXP, src: SEXP) {
    unsafe {
        if src.is_null() || src == R_NilValue() {
            return;
        }
        let mut attr = crate::sexp::accessors::ATTRIB(src);
        while !attr.is_null() && attr != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(dst, TAG(attr), CAR(attr));
            attr = CDR(attr);
        }
    }
}

/// Evaluate a d/p/q builtin over `operands` with stock recycling.
///
/// Result length is the max operand length; every operand recycles modulo
/// its length; any zero-length operand yields a zero-length result (with
/// the main operand's attributes when it is the empty one, SETUP_MathN);
/// the result carries the attributes of the first operand whose length
/// matches (FINISH_MathN). `NA` inputs propagate as `NA`, `NaN` inputs as
/// `NaN`, and newly produced `NaN`s warn "NaNs produced" once.
unsafe fn dpq_evaluate(
    call: SEXP,
    operands: &[DpqOperand],
    lower_tail: bool,
    log_p: bool,
    compute: &mut dyn FnMut(&[f64], bool, bool) -> f64,
) -> SEXP {
    unsafe {
        // Mathlib warnings raised inside `compute` attribute to this call
        // (upstream resolves it by walking out of the builtin context).
        let _mathlib_call = crate::mainutils::errors::mathlib_warning_call_guard(call);
        for (k, op) in operands.iter().enumerate() {
            if dpq_operand_len(op) == 0 {
                let empty = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
                if empty.is_null() {
                    return R_NilValue();
                }
                if k == 0 {
                    if let DpqOperand::Numeric(s) | DpqOperand::Inverse(s) = op {
                        dpq_copy_attrib(empty, *s);
                    }
                }
                return empty;
            }
        }
        let n = operands.iter().map(dpq_operand_len).max().unwrap_or(0);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        let mut vals = vec![0.0; operands.len()];
        let mut naflag = false;
        for i in 0..n {
            for (k, op) in operands.iter().enumerate() {
                vals[k] = match op {
                    DpqOperand::Default(d) => *d,
                    DpqOperand::Numeric(s) => elt_real_safe(*s, i),
                    DpqOperand::Inverse(s) => 1.0 / elt_real_safe(*s, i),
                };
            }
            // if_NA_MathN_set: NA in -> NA, NaN in -> NaN, else compute.
            let out = if vals
                .iter()
                .any(|v| v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN)
            {
                crate::sexp::ffi::NA_REAL
            } else if vals.iter().any(|v| v.is_nan()) {
                f64::NAN
            } else {
                let v = compute(&vals, lower_tail, log_p);
                if v.is_nan() {
                    naflag = true;
                }
                v
            };
            *dst.add(i as usize) = out;
        }
        if naflag {
            crate::mainutils::errors::Rf_warningcall1(call, c"NaNs produced".as_ptr());
        }
        for op in operands {
            if dpq_operand_len(op) == n {
                if let DpqOperand::Numeric(s) | DpqOperand::Inverse(s) = op {
                    dpq_copy_attrib(result, *s);
                }
                break;
            }
        }
        result
    }
}

/// Resolve the standard dpq operand list: the main argument (slot 0,
/// required) plus one parameter formal per entry of `defaults` (slots
/// `1..`), all with stock non-numeric checks.
unsafe fn dpq_operands(
    args: SEXP,
    names: &[&str],
    defaults: &[Option<f64>],
) -> (Vec<DpqOperand>, Vec<SEXP>, Vec<bool>) {
    unsafe {
        let (slots, filled) = dist_match(args, names);
        let mut ops = Vec::with_capacity(defaults.len() + 1);
        ops.push(dpq_num(&slots, &filled, names, 0, None, DPQ_NONNUM));
        for (k, d) in defaults.iter().enumerate() {
            ops.push(dpq_num(&slots, &filled, names, k + 1, *d, DPQ_NONNUM));
        }
        (ops, slots, filled)
    }
}

// ---------------------------------------------------------------------------
// Distribution functions: dnorm, pnorm, qnorm, dpois, ppois
// ---------------------------------------------------------------------------

/// R's `dnorm(x, mean=0, sd=1, log=FALSE)` — normal density.
pub unsafe fn do_dnorm(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "mean", "sd", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::normal::dnorm4_inner(v[0], v[1], v[2], lp)
        })
    }
}

/// R's `pnorm(q, mean=0, sd=1, lower.tail=TRUE, log.p=FALSE)` — normal CDF.
pub unsafe fn do_pnorm(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "mean", "sd", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::normal::pnorm5_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `qnorm(p, mean=0, sd=1, lower.tail=TRUE, log.p=FALSE)` — normal quantile.
pub unsafe fn do_qnorm(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "mean", "sd", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::normal::qnorm5_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `dpois(x, lambda, log=FALSE)` — Poisson density.
pub unsafe fn do_dpois(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "lambda", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(1.0)]);
        let log_p = dpq_flag(&slots, &filled, 2, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::poisson::dpois_inner(v[0], v[1], lp)
        })
    }
}

/// R's `ppois(q, lambda, lower.tail=TRUE, log.p=FALSE)` — Poisson CDF.
pub unsafe fn do_ppois(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "lambda", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 2, true);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::poisson::ppois_inner(v[0], v[1], lt, lp)
        })
    }
}

/// R's `qpois(p, lambda, lower.tail=TRUE, log.p=FALSE)` — Poisson quantile.
pub unsafe fn do_qpois(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "lambda", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 2, true);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::poisson::qpois_inner(v[0], v[1], lt, lp)
        })
    }
}

/// R's `dbinom(x, size, prob, log=FALSE)` — binomial density.
pub unsafe fn do_dbinom(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "size", "prob", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None]);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::binomial::dbinom_inner(v[0], v[1], v[2], lp)
        })
    }
}

/// R's `pbinom(q, size, prob, lower.tail=TRUE, log.p=FALSE)` — binomial CDF.
pub unsafe fn do_pbinom(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "size", "prob", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::binomial::pbinom_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `qbinom(p, size, prob, lower.tail=TRUE, log.p=FALSE)` — binomial quantile.
pub unsafe fn do_qbinom(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "size", "prob", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::binomial::qbinom_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `dexp(x, rate=1, log=FALSE)` — exponential density. The R wrapper
/// converts `rate` to `scale` with a binary operation (`1/rate`), so the
/// conversion recycles per element and uses binary-op error semantics.
pub unsafe fn do_dexp(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "rate", "log"];
        let (slots, filled) = dist_match(args, &names);
        let x = dpq_num(&slots, &filled, &names, 0, None, DPQ_NONNUM);
        let scale = dpq_inverse(&slots, &filled, 1).unwrap_or(DpqOperand::Default(1.0));
        let log_p = dpq_flag(&slots, &filled, 2, false);
        dpq_evaluate(call, &[x, scale], false, log_p, &mut |v, _, lp| {
            crate::dist::exponential::dexp_inner(v[0], v[1], lp)
        })
    }
}

/// R's `pexp(q, rate=1, lower.tail=TRUE, log.p=FALSE)` — exponential CDF.
pub unsafe fn do_pexp(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "rate", "lower.tail", "log.p"];
        let (slots, filled) = dist_match(args, &names);
        let q = dpq_num(&slots, &filled, &names, 0, None, DPQ_NONNUM);
        let scale = dpq_inverse(&slots, &filled, 1).unwrap_or(DpqOperand::Default(1.0));
        let lower_tail = dpq_flag(&slots, &filled, 2, true);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &[q, scale], lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::exponential::pexp_inner(v[0], v[1], lt, lp)
        })
    }
}

/// R's `qexp(p, rate=1, lower.tail=TRUE, log.p=FALSE)` — exponential quantile.
pub unsafe fn do_qexp(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "rate", "lower.tail", "log.p"];
        let (slots, filled) = dist_match(args, &names);
        let p = dpq_num(&slots, &filled, &names, 0, None, DPQ_NONNUM);
        let scale = dpq_inverse(&slots, &filled, 1).unwrap_or(DpqOperand::Default(1.0));
        let lower_tail = dpq_flag(&slots, &filled, 2, true);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &[p, scale], lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::exponential::qexp_inner(v[0], v[1], lt, lp)
        })
    }
}

// ---------------------------------------------------------------------------
// Distribution functions: gamma, beta, t, chisq, cauchy, weibull, f, nbinom, geom
// ---------------------------------------------------------------------------

/// The gamma-family rate/scale conflict handling from the stats R wrappers:
/// supplying both warns when they agree (`|rate*scale - 1| < 1e-15`) and
/// stops otherwise.
unsafe fn gamma_rate_scale_conflict(slots: &[SEXP], filled: &[bool]) {
    unsafe {
        if dpq_supplied(slots, filled, 2) && dpq_supplied(slots, filled, 3) {
            let rate = real_or_default(slots[2], 1.0);
            let scale = real_or_default(slots[3], 1.0);
            if (rate * scale - 1.0).abs() < 1e-15 {
                crate::mainutils::errors::Rf_warning1(
                    c"specify 'rate' or 'scale' but not both".as_ptr(),
                );
            } else {
                base_error("specify 'rate' or 'scale' but not both");
            }
        }
    }
}

/// The gamma-family scale operand: supplied `scale` wins, else the
/// R-level `1/rate` conversion, else 1.
unsafe fn gamma_scale_operand(slots: &[SEXP], filled: &[bool], names: &[&str]) -> DpqOperand {
    unsafe {
        if dpq_supplied(slots, filled, 3) {
            dpq_num(slots, filled, names, 3, Some(1.0), DPQ_NONNUM)
        } else if let Some(inv) = dpq_inverse(slots, filled, 2) {
            inv
        } else {
            DpqOperand::Default(1.0)
        }
    }
}

/// R's `dgamma(x, shape, rate=1, scale=1/rate, log=FALSE)` — gamma density.
pub unsafe fn do_dgamma(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "shape", "rate", "scale", "log"];
        let (slots, filled) = dist_match(args, &names);
        gamma_rate_scale_conflict(&slots, &filled);
        let x = dpq_num(&slots, &filled, &names, 0, None, DPQ_NONNUM);
        let shape = dpq_num(&slots, &filled, &names, 1, None, DPQ_NONNUM);
        let scale = gamma_scale_operand(&slots, &filled, &names);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &[x, shape, scale], false, log_p, &mut |v, _, lp| {
            crate::dist::gamma::dgamma_inner(v[0], v[1], v[2], lp)
        })
    }
}

/// R's `pgamma(q, shape, rate=1, scale=1/rate, lower.tail=TRUE, log.p=FALSE)`.
pub unsafe fn do_pgamma(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "shape", "rate", "scale", "lower.tail", "log.p"];
        let (slots, filled) = dist_match(args, &names);
        gamma_rate_scale_conflict(&slots, &filled);
        let q = dpq_num(&slots, &filled, &names, 0, None, DPQ_NONNUM);
        let shape = dpq_num(&slots, &filled, &names, 1, None, DPQ_NONNUM);
        let scale = gamma_scale_operand(&slots, &filled, &names);
        let lower_tail = dpq_flag(&slots, &filled, 4, true);
        let log_p = dpq_flag(&slots, &filled, 5, false);
        dpq_evaluate(
            call,
            &[q, shape, scale],
            lower_tail,
            log_p,
            &mut |v, lt, lp| crate::dist::gamma::pgamma_inner(v[0], v[1], v[2], lt, lp),
        )
    }
}

/// R's `qgamma(p, shape, rate=1, scale=1/rate, lower.tail=TRUE, log.p=FALSE)`.
pub unsafe fn do_qgamma(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "shape", "rate", "scale", "lower.tail", "log.p"];
        let (slots, filled) = dist_match(args, &names);
        gamma_rate_scale_conflict(&slots, &filled);
        let p = dpq_num(&slots, &filled, &names, 0, None, DPQ_NONNUM);
        let shape = dpq_num(&slots, &filled, &names, 1, None, DPQ_NONNUM);
        let scale = gamma_scale_operand(&slots, &filled, &names);
        let lower_tail = dpq_flag(&slots, &filled, 4, true);
        let log_p = dpq_flag(&slots, &filled, 5, false);
        dpq_evaluate(
            call,
            &[p, shape, scale],
            lower_tail,
            log_p,
            &mut |v, lt, lp| crate::dist::gamma::qgamma_inner(v[0], v[1], v[2], lt, lp),
        )
    }
}

/// R's `dbeta(x, shape1, shape2, log=FALSE)` — beta density.
pub unsafe fn do_dbeta(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "shape1", "shape2", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None]);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::beta::dbeta_inner(v[0], v[1], v[2], lp)
        })
    }
}

/// R's `pbeta(q, shape1, shape2, lower.tail=TRUE, log.p=FALSE)` — beta CDF.
pub unsafe fn do_pbeta(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "shape1", "shape2", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::beta::pbeta_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `qbeta(p, shape1, shape2, lower.tail=TRUE, log.p=FALSE)` — beta quantile.
pub unsafe fn do_qbeta(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "shape1", "shape2", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::beta::qbeta_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `dt(x, df, ncp, log=FALSE)` — t density. Supplying `ncp` switches to
/// the noncentral variant, like stock's `dt()` wrapper.
pub unsafe fn do_dt(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "df", "ncp", "log"];
        let (slots, filled) = dist_match(args, &names);
        let x = dpq_num(&slots, &filled, &names, 0, None, DPQ_NONNUM);
        let df = dpq_num(&slots, &filled, &names, 1, None, DPQ_NONNUM);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        if dpq_supplied(&slots, &filled, 2) {
            let ncp = dpq_num(&slots, &filled, &names, 2, None, DPQ_NONNUM);
            dpq_evaluate(call, &[x, df, ncp], false, log_p, &mut |v, _, lp| {
                crate::dist::nt_dist::dnt_inner(v[0], v[1], v[2], lp)
            })
        } else {
            dpq_evaluate(call, &[x, df], false, log_p, &mut |v, _, lp| {
                crate::dist::t_dist::dt_inner(v[0], v[1], lp)
            })
        }
    }
}

/// R's `pt(q, df, ncp, lower.tail=TRUE, log.p=FALSE)` — t CDF.
pub unsafe fn do_pt(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "df", "ncp", "lower.tail", "log.p"];
        let (slots, filled) = dist_match(args, &names);
        let q = dpq_num(&slots, &filled, &names, 0, None, DPQ_NONNUM);
        let df = dpq_num(&slots, &filled, &names, 1, None, DPQ_NONNUM);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        if dpq_supplied(&slots, &filled, 2) {
            let ncp = dpq_num(&slots, &filled, &names, 2, None, DPQ_NONNUM);
            dpq_evaluate(call, &[q, df, ncp], lower_tail, log_p, &mut |v, lt, lp| {
                crate::dist::nt_dist::pnt_inner(v[0], v[1], v[2], lt, lp)
            })
        } else {
            dpq_evaluate(call, &[q, df], lower_tail, log_p, &mut |v, lt, lp| {
                crate::dist::t_dist::pt_inner(v[0], v[1], lt, lp)
            })
        }
    }
}

/// R's `qt(p, df, ncp, lower.tail=TRUE, log.p=FALSE)` — t quantile.
pub unsafe fn do_qt(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "df", "ncp", "lower.tail", "log.p"];
        let (slots, filled) = dist_match(args, &names);
        let p = dpq_num(&slots, &filled, &names, 0, None, DPQ_NONNUM);
        let df = dpq_num(&slots, &filled, &names, 1, None, DPQ_NONNUM);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        if dpq_supplied(&slots, &filled, 2) {
            let ncp = dpq_num(&slots, &filled, &names, 2, None, DPQ_NONNUM);
            dpq_evaluate(call, &[p, df, ncp], lower_tail, log_p, &mut |v, lt, lp| {
                crate::dist::nt_dist::qnt_inner(v[0], v[1], v[2], lt, lp)
            })
        } else {
            dpq_evaluate(call, &[p, df], lower_tail, log_p, &mut |v, lt, lp| {
                crate::dist::t_dist::qt_inner(v[0], v[1], lt, lp)
            })
        }
    }
}

/// R's `dchisq(x, df, log=FALSE)` — chi-squared density.
pub unsafe fn do_dchisq(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "df", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None]);
        let log_p = dpq_flag(&slots, &filled, 2, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::chisq::dchisq_inner(v[0], v[1], lp)
        })
    }
}

/// R's `pchisq(q, df, lower.tail=TRUE, log.p=FALSE)` — chi-squared CDF.
pub unsafe fn do_pchisq(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "df", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None]);
        let lower_tail = dpq_flag(&slots, &filled, 2, true);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::chisq::pchisq_inner(v[0], v[1], lt, lp)
        })
    }
}

/// R's `qchisq(p, df, lower.tail=TRUE, log.p=FALSE)` — chi-squared quantile.
pub unsafe fn do_qchisq(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "df", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None]);
        let lower_tail = dpq_flag(&slots, &filled, 2, true);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::chisq::qchisq_inner(v[0], v[1], lt, lp)
        })
    }
}

/// R's `dcauchy(x, location=0, scale=1, log=FALSE)` — Cauchy density.
pub unsafe fn do_dcauchy(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "location", "scale", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::cauchy::dcauchy_inner(v[0], v[1], v[2], lp)
        })
    }
}

/// R's `pcauchy(q, location=0, scale=1, lower.tail=TRUE, log.p=FALSE)` — Cauchy CDF.
pub unsafe fn do_pcauchy(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "location", "scale", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::cauchy::pcauchy_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `qcauchy(p, location=0, scale=1, lower.tail=TRUE, log.p=FALSE)` — Cauchy quantile.
pub unsafe fn do_qcauchy(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "location", "scale", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::cauchy::qcauchy_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `dweibull(x, shape, scale=1, log=FALSE)` — Weibull density.
pub unsafe fn do_dweibull(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "shape", "scale", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, Some(1.0)]);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::weibull::dweibull_inner(v[0], v[1], v[2], lp)
        })
    }
}

/// R's `pweibull(q, shape, scale=1, lower.tail=TRUE, log.p=FALSE)` — Weibull CDF.
pub unsafe fn do_pweibull(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "shape", "scale", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::weibull::pweibull_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `qweibull(p, shape, scale=1, lower.tail=TRUE, log.p=FALSE)` — Weibull quantile.
pub unsafe fn do_qweibull(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "shape", "scale", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::weibull::qweibull_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `df(x, df1, df2, log=FALSE)` — F distribution density.
pub unsafe fn do_df(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "df1", "df2", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None]);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::f_dist::df_inner(v[0], v[1], v[2], lp)
        })
    }
}

/// R's `pf(q, df1, df2, lower.tail=TRUE, log.p=FALSE)` — F distribution CDF.
pub unsafe fn do_pf(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "df1", "df2", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::f_dist::pf_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `qf(p, df1, df2, lower.tail=TRUE, log.p=FALSE)` — F distribution quantile.
pub unsafe fn do_qf(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "df1", "df2", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::f_dist::qf_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `dunif(x, min=0, max=1, log=FALSE)` — uniform density.
pub unsafe fn do_dunif(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "min", "max", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::uniform::dunif_inner(v[0], v[1], v[2], lp)
        })
    }
}

/// R's `punif(q, min=0, max=1, lower.tail=TRUE, log.p=FALSE)` — uniform CDF.
pub unsafe fn do_punif(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "min", "max", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::uniform::punif_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `qunif(p, min=0, max=1, lower.tail=TRUE, log.p=FALSE)` — uniform quantile.
pub unsafe fn do_qunif(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "min", "max", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::uniform::qunif_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `dnbinom(x, size, prob, mu, log=FALSE)` — negative binomial density.
/// The stats wrapper dispatches to the mu variant when `mu` is supplied
/// (erroring if `prob` is also given); `prob` is required otherwise.
pub unsafe fn do_dnbinom(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "size", "prob", "mu", "log"];
        let (slots, filled) = dist_match(args, &names);
        let x = dpq_num(&slots, &filled, &names, 0, None, DPQ_NONNUM);
        let size = dpq_num(&slots, &filled, &names, 1, None, DPQ_NONNUM);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        if dpq_supplied(&slots, &filled, 3) {
            if dpq_supplied(&slots, &filled, 2) {
                base_error("'prob' and 'mu' both specified");
            }
            let mu = dpq_num(&slots, &filled, &names, 3, None, DPQ_NONNUM);
            dpq_evaluate(call, &[x, size, mu], false, log_p, &mut |v, _, lp| {
                let prob = v[1] / (v[1] + v[2]);
                crate::dist::nbinom::dnbinom_inner(v[0], v[1], prob, lp)
            })
        } else {
            let prob = dpq_num(&slots, &filled, &names, 2, None, DPQ_NONNUM);
            dpq_evaluate(call, &[x, size, prob], false, log_p, &mut |v, _, lp| {
                crate::dist::nbinom::dnbinom_inner(v[0], v[1], v[2], lp)
            })
        }
    }
}

/// R's `pnbinom(q, size, prob, mu, lower.tail=TRUE, log.p=FALSE)` — negative binomial CDF.
pub unsafe fn do_pnbinom(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "size", "prob", "mu", "lower.tail", "log.p"];
        let (slots, filled) = dist_match(args, &names);
        let q = dpq_num(&slots, &filled, &names, 0, None, DPQ_NONNUM);
        let size = dpq_num(&slots, &filled, &names, 1, None, DPQ_NONNUM);
        let lower_tail = dpq_flag(&slots, &filled, 4, true);
        let log_p = dpq_flag(&slots, &filled, 5, false);
        if dpq_supplied(&slots, &filled, 3) {
            if dpq_supplied(&slots, &filled, 2) {
                base_error("'prob' and 'mu' both specified");
            }
            let mu = dpq_num(&slots, &filled, &names, 3, None, DPQ_NONNUM);
            dpq_evaluate(call, &[q, size, mu], lower_tail, log_p, &mut |v, lt, lp| {
                let prob = v[1] / (v[1] + v[2]);
                crate::dist::nbinom::pnbinom_inner(v[0], v[1], prob, lt, lp)
            })
        } else {
            let prob = dpq_num(&slots, &filled, &names, 2, None, DPQ_NONNUM);
            dpq_evaluate(
                call,
                &[q, size, prob],
                lower_tail,
                log_p,
                &mut |v, lt, lp| crate::dist::nbinom::pnbinom_inner(v[0], v[1], v[2], lt, lp),
            )
        }
    }
}

/// R's `qnbinom(p, size, prob, mu, lower.tail=TRUE, log.p=FALSE)` — negative binomial quantile.
pub unsafe fn do_qnbinom(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "size", "prob", "mu", "lower.tail", "log.p"];
        let (slots, filled) = dist_match(args, &names);
        let p = dpq_num(&slots, &filled, &names, 0, None, DPQ_NONNUM);
        let size = dpq_num(&slots, &filled, &names, 1, None, DPQ_NONNUM);
        let lower_tail = dpq_flag(&slots, &filled, 4, true);
        let log_p = dpq_flag(&slots, &filled, 5, false);
        if dpq_supplied(&slots, &filled, 3) {
            if dpq_supplied(&slots, &filled, 2) {
                base_error("'prob' and 'mu' both specified");
            }
            let mu = dpq_num(&slots, &filled, &names, 3, None, DPQ_NONNUM);
            dpq_evaluate(call, &[p, size, mu], lower_tail, log_p, &mut |v, lt, lp| {
                let prob = v[1] / (v[1] + v[2]);
                crate::dist::nbinom::qnbinom_inner(v[0], v[1], prob, lt, lp)
            })
        } else {
            let prob = dpq_num(&slots, &filled, &names, 2, None, DPQ_NONNUM);
            dpq_evaluate(
                call,
                &[p, size, prob],
                lower_tail,
                log_p,
                &mut |v, lt, lp| crate::dist::nbinom::qnbinom_inner(v[0], v[1], v[2], lt, lp),
            )
        }
    }
}

/// R's `dgeom(x, prob, log=FALSE)` — geometric density.
pub unsafe fn do_dgeom(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "prob", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None]);
        let log_p = dpq_flag(&slots, &filled, 2, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::geometric::dgeom_inner(v[0], v[1], lp)
        })
    }
}

/// R's `pgeom(q, prob, lower.tail=TRUE, log.p=FALSE)` — geometric CDF.
pub unsafe fn do_pgeom(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "prob", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None]);
        let lower_tail = dpq_flag(&slots, &filled, 2, true);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::geometric::pgeom_inner(v[0], v[1], lt, lp)
        })
    }
}

/// R's `qgeom(p, prob, lower.tail=TRUE, log.p=FALSE)` — geometric quantile.
pub unsafe fn do_qgeom(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "prob", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None]);
        let lower_tail = dpq_flag(&slots, &filled, 2, true);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::geometric::qgeom_inner(v[0], v[1], lt, lp)
        })
    }
}

// ---------------------------------------------------------------------------
// Distribution functions: lnorm, logistic, signrank, wilcox, hyper, tukey
// ---------------------------------------------------------------------------

/// R's `dlnorm(x, meanlog=0, sdlog=1, log=FALSE)` — lognormal density.
pub unsafe fn do_dlnorm(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "meanlog", "sdlog", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::lnorm::dlnorm_inner(v[0], v[1], v[2], lp)
        })
    }
}

/// R's `plnorm(q, meanlog=0, sdlog=1, lower.tail=TRUE, log.p=FALSE)` — lognormal CDF.
pub unsafe fn do_plnorm(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "meanlog", "sdlog", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::lnorm::plnorm_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `qlnorm(p, meanlog=0, sdlog=1, lower.tail=TRUE, log.p=FALSE)` — lognormal quantile.
pub unsafe fn do_qlnorm(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "meanlog", "sdlog", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::lnorm::qlnorm_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `dlogis(x, location=0, scale=1, log=FALSE)` — logistic density.
pub unsafe fn do_dlogis(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "location", "scale", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::logistic::dlogis_inner(v[0], v[1], v[2], lp)
        })
    }
}

/// R's `plogis(q, location=0, scale=1, lower.tail=TRUE, log.p=FALSE)` — logistic CDF.
pub unsafe fn do_plogis(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "location", "scale", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::logistic::plogis_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `qlogis(p, location=0, scale=1, lower.tail=TRUE, log.p=FALSE)` — logistic quantile.
pub unsafe fn do_qlogis(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "location", "scale", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[Some(0.0), Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::logistic::qlogis_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `dsignrank(x, n, log=FALSE)` — Wilcoxon signed rank density.
pub unsafe fn do_dsignrank(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "n", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None]);
        let log_p = dpq_flag(&slots, &filled, 2, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::signrank::dsignrank_inner(v[0], v[1], lp)
        })
    }
}

/// R's `psignrank(q, n, lower.tail=TRUE, log.p=FALSE)` — Wilcoxon signed rank CDF.
pub unsafe fn do_psignrank(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "n", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None]);
        let lower_tail = dpq_flag(&slots, &filled, 2, true);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::signrank::psignrank_inner(v[0], v[1], lt, lp)
        })
    }
}

/// R's `qsignrank(p, n, lower.tail=TRUE, log.p=FALSE)` — Wilcoxon signed rank quantile.
pub unsafe fn do_qsignrank(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "n", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None]);
        let lower_tail = dpq_flag(&slots, &filled, 2, true);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::signrank::qsignrank_inner(v[0], v[1], lt, lp)
        })
    }
}

/// R's `dwilcox(x, m, n, log=FALSE)` — Wilcoxon rank sum density.
pub unsafe fn do_dwilcox(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "m", "n", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None]);
        let log_p = dpq_flag(&slots, &filled, 3, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::wilcox::dwilcox_inner(v[0], v[1], v[2], lp)
        })
    }
}

/// R's `pwilcox(q, m, n, lower.tail=TRUE, log.p=FALSE)` — Wilcoxon rank sum CDF.
pub unsafe fn do_pwilcox(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "m", "n", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::wilcox::pwilcox_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `qwilcox(p, m, n, lower.tail=TRUE, log.p=FALSE)` — Wilcoxon rank sum quantile.
pub unsafe fn do_qwilcox(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "m", "n", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None]);
        let lower_tail = dpq_flag(&slots, &filled, 3, true);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::wilcox::qwilcox_inner(v[0], v[1], v[2], lt, lp)
        })
    }
}

/// R's `dhyper(x, m, n, k, log=FALSE)` — hypergeometric density (4 params).
pub unsafe fn do_dhyper(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["x", "m", "n", "k", "log"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None, None]);
        let log_p = dpq_flag(&slots, &filled, 4, false);
        dpq_evaluate(call, &ops, false, log_p, &mut |v, _, lp| {
            crate::dist::hypergeometric::dhyper_inner(v[0], v[1], v[2], v[3], lp)
        })
    }
}

/// R's `phyper(q, m, n, k, lower.tail=TRUE, log.p=FALSE)` — hypergeometric CDF (4 params).
pub unsafe fn do_phyper(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "m", "n", "k", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None, None]);
        let lower_tail = dpq_flag(&slots, &filled, 4, true);
        let log_p = dpq_flag(&slots, &filled, 5, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::hypergeometric::phyper_inner(v[0], v[1], v[2], v[3], lt, lp)
        })
    }
}

/// R's `qhyper(p, m, n, k, lower.tail=TRUE, log.p=FALSE)` — hypergeometric quantile (4 params).
pub unsafe fn do_qhyper(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "m", "n", "k", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None, None]);
        let lower_tail = dpq_flag(&slots, &filled, 4, true);
        let log_p = dpq_flag(&slots, &filled, 5, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::hypergeometric::qhyper_inner(v[0], v[1], v[2], v[3], lt, lp)
        })
    }
}

/// R's `ptukey(q, nmeans, df, nranges=1, lower.tail=TRUE, log.p=FALSE)` —
/// Studentized range CDF.
pub unsafe fn do_ptukey(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["q", "nmeans", "df", "nranges", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None, Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 4, true);
        let log_p = dpq_flag(&slots, &filled, 5, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::tukey::ptukey_inner(v[0], v[3], v[1], v[2], lt, lp)
        })
    }
}

/// R's `qtukey(p, nmeans, df, nranges=1, lower.tail=TRUE, log.p=FALSE)` —
/// Studentized range quantile.
pub unsafe fn do_qtukey(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = ["p", "nmeans", "df", "nranges", "lower.tail", "log.p"];
        let (ops, slots, filled) = dpq_operands(args, &names, &[None, None, Some(1.0)]);
        let lower_tail = dpq_flag(&slots, &filled, 4, true);
        let log_p = dpq_flag(&slots, &filled, 5, false);
        dpq_evaluate(call, &ops, lower_tail, log_p, &mut |v, lt, lp| {
            crate::dist::tukey::qtukey_inner(v[0], v[3], v[1], v[2], lt, lp)
        })
    }
}

/// R's `dmultinom(x, prob, log=FALSE)` — multinomial probability.
pub unsafe fn do_dmultinom(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let (m, _filled) = dist_match(args, &["x", "prob", "log"]);
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
