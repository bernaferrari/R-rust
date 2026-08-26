//! R-level RNG builtins — ports R's src/main/RNG.c dispatch layer.
//!
//! Provides the R built-in functions: set.seed(), RNGkind(), runif(), rnorm(),
//! rpois(), rbinom(), rexp(). These sit on top of the nmath layer (which has
//! the actual generator implementations).

use std::ffi::{CStr, CString};
use std::os::raw::c_int;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    CAR, CDR, CHAR, INTEGER, LENGTH, LOGICAL, PRINTNAME, REAL, SET_STRING_ELT, SET_VECTOR_ELT,
    STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector3};
use crate::sexp::ffi::{NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// RNG kind tracking
// ---------------------------------------------------------------------------

/// Current RNG kind (0 = Marsaglia-MultiCarry default).
/// Get the current RNG kind.
pub fn get_rng_kind() -> i32 {
    crate::sexp::instance::with_required_current_instance(|instance| instance.rng_kind)
}

/// Set the RNG kind.
pub fn set_rng_kind(kind: i32) {
    crate::sexp::instance::with_required_current_instance(|instance| {
        instance.rng_kind = kind;
    });
}

// ---------------------------------------------------------------------------
// do_set.seed
// ---------------------------------------------------------------------------

/// Handle R's `set.seed(seed, kind, normal.kind)`.
///
/// Sets the RNG state from an integer seed. If seed is NA_INTEGER or NULL,
/// uses a time-based seed.
pub unsafe fn do_set_seed(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let seed_arg = CAR(args);
        let kind_arg = CAR(CDR(args));

        // Handle seed
        if !seed_arg.is_null() && seed_arg != R_NilValue() {
            let t = TYPEOF(seed_arg);
            if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let s = *INTEGER(seed_arg);
                if s != crate::sexp::ffi::NA_INTEGER {
                    // Use seed to set RNG state
                    // Split seed into two 16-bit halves for Marsaglia-MultiCarry
                    let i1 = (s as u32).wrapping_mul(69069).wrapping_add(1);
                    let i2 = (s as u32).wrapping_mul(12345).wrapping_add(67890);
                    crate::rng::set_seed(i1, i2);
                }
            } else if t == SEXPTYPE::REALSXP {
                let s = *REAL(seed_arg);
                if !s.is_nan() {
                    let is = s as i64;
                    let i1 = (is as u32).wrapping_mul(69069).wrapping_add(1);
                    let i2 = (is as u32).wrapping_mul(12345).wrapping_add(67890);
                    crate::rng::set_seed(i1, i2);
                }
            }
        } else {
            // No seed given — use a pseudo-random default
            // In a real implementation, this would use the system clock
            crate::rng::set_seed(1234, 5678);
        }

        // Handle kind
        if !kind_arg.is_null() && kind_arg != R_NilValue() {
            let t = TYPEOF(kind_arg);
            if t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP {
                let k = if t == SEXPTYPE::INTSXP {
                    *INTEGER(kind_arg)
                } else {
                    *REAL(kind_arg) as i32
                };
                set_rng_kind(k);
            }
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_RNGkind
// ---------------------------------------------------------------------------

/// Handle R's `RNGkind(kind, normal.kind)`.
///
/// Without arguments, returns the current RNG kind as a character vector.
/// With arguments, sets the RNG kind.
pub unsafe fn do_RNGkind(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let kind_arg = CAR(args);

        if kind_arg.is_null() || kind_arg == R_NilValue() {
            // No arguments — return current kind
            let kind_name = match get_rng_kind() {
                0 => "Marsaglia-MultiCarry",
                1 => "Wichmann-Hill",
                2 => "Mersenne-Twister",
                _ => "Marsaglia-MultiCarry",
            };
            let cstr = CString::new(kind_name).unwrap_or_default();
            return crate::sexp::constructors::Rf_mkString(cstr.as_ptr());
        }

        // Set kind
        let t = TYPEOF(kind_arg);
        if t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP {
            let k = if t == SEXPTYPE::INTSXP {
                *INTEGER(kind_arg)
            } else {
                *REAL(kind_arg) as i32
            };
            set_rng_kind(k.max(0).min(2));
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_runif
// ---------------------------------------------------------------------------

/// Handle R's `runif(n, min, max)`.
///
/// Generates n uniform random numbers in [min, max).
pub unsafe fn do_runif(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let min_arg = CAR(CDR(args));
        let max_arg = CAR(CDR(CDR(args)));

        let n = parse_n(n_arg, 1);
        let min = parse_double_scalar(min_arg, 0.0);
        let max = parse_double_scalar(max_arg, 1.0);

        if n <= 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);

        let range = max - min;
        for i in 0..n {
            let u = crate::rng::unif_rand();
            *dst.add(i as usize) = min + u * range;
        }

        result
    }
}

// ---------------------------------------------------------------------------
// do_rnorm
// ---------------------------------------------------------------------------

/// Handle R's `rnorm(n, mean, sd)`.
///
/// Generates n normal random numbers.
pub unsafe fn do_rnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let mean_arg = CAR(CDR(args));
        let sd_arg = CAR(CDR(CDR(args)));

        let n = parse_n(n_arg, 1);
        let mu = parse_double_scalar(mean_arg, 0.0);
        let sigma = parse_double_scalar(sd_arg, 1.0);

        if n <= 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            *dst.add(i as usize) = crate::dist::normal::rnorm(mu, sigma);
        }

        result
    }
}

// ---------------------------------------------------------------------------
// do_rpois
// ---------------------------------------------------------------------------

/// Handle R's `rpois(n, lambda)`.
///
/// Generates n Poisson random numbers.
pub unsafe fn do_rpois(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let lambda_arg = CAR(CDR(args));

        let n = parse_n(n_arg, 1);
        let lambda = parse_double_scalar(lambda_arg, 1.0);

        if n <= 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            *dst.add(i as usize) = crate::dist::poisson::rpois(lambda);
        }

        result
    }
}

// ---------------------------------------------------------------------------
// do_rexp
// ---------------------------------------------------------------------------

/// Handle R's `rexp(n, rate)`.
///
/// Generates n exponential random numbers with mean = 1/rate.
pub unsafe fn do_rexp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let rate_arg = CAR(CDR(args));

        let n = parse_n(n_arg, 1);
        let rate = parse_double_scalar(rate_arg, 1.0);
        let scale = if rate > 0.0 { 1.0 / rate } else { NA_REAL };

        if n <= 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            *dst.add(i as usize) = crate::dist::exponential::rexp_inner(scale);
        }

        result
    }
}

// ---------------------------------------------------------------------------
// do_sample
// ---------------------------------------------------------------------------

/// Handle R's `sample(x, size, replace, prob)`.
pub unsafe fn do_sample(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let parsed = sample_args(args);

        if is_missing_or_null(parsed.x) {
            return R_NilValue();
        }

        if let Some(n) = sample_int_shortcut_n(parsed.x) {
            // Upstream base::sample's scalar branch calls
            // sample.int(x, size, replace, prob); errors surface attributed
            // to that call.
            let sample_int_call = sample_int_wrapper_call("x");
            return crate::mainutils::errors::attribute_handler_errors(sample_int_call, || {
                sample_int_values(n, parsed.size, parsed.replace, parsed.prob)
            });
        }

        let x_len = XLENGTH(parsed.x);
        let size = parse_n(parsed.size, x_len as c_int);
        let replace = parse_replace(parsed.replace);
        // Upstream base::sample's vector branch calls
        // x[sample.int(length(x), size, replace, prob)]; errors surface
        // attributed to that inner call.
        let sample_int_call = sample_int_wrapper_call("length(x)");
        crate::mainutils::errors::attribute_handler_errors(sample_int_call, || {
            let indices = if is_present(parsed.prob) {
                let weights = parse_probability_weights(parsed.prob, x_len);
                weighted_sample_indices(&weights, size, replace)
            } else {
                sample_indices(x_len, size, replace)
            };
            sample_vector_by_indices(parsed.x, &indices)
        })
    }
}

/// Build `sample.int(<first-arg>, size, replace, prob)` — the wrapper call
/// base::sample makes in its R body — for error attribution.
unsafe fn sample_int_wrapper_call(first_arg: &str) -> SEXP {
    unsafe {
        let sym = |name: &str| {
            crate::sexp::symbol::Rf_install(
                std::ffi::CString::new(name).unwrap_or_default().as_ptr(),
            )
        };
        let first = if first_arg == "length(x)" {
            crate::sexp::constructors::Rf_lang2(sym("length"), sym("x"))
        } else {
            sym(first_arg)
        };
        let nil = R_NilValue();
        let prob = crate::sexp::constructors::Rf_cons(sym("prob"), nil);
        let replace = crate::sexp::constructors::Rf_cons(sym("replace"), prob);
        let size = crate::sexp::constructors::Rf_cons(sym("size"), replace);
        let args = crate::sexp::constructors::Rf_cons(first, size);
        let call = crate::sexp::constructors::Rf_cons(sym("sample.int"), args);
        if !call.is_null() {
            (*call)
                .sxpinfo
                .set_type(crate::sexp::ffi::SEXPTYPE::LANGSXP);
        }
        call
    }
}

#[derive(Clone, Copy)]
struct SampleArgs {
    x: SEXP,
    size: SEXP,
    replace: SEXP,
    prob: SEXP,
}

unsafe fn sample_args(args: SEXP) -> SampleArgs {
    unsafe {
        let mut parsed = SampleArgs {
            x: R_NilValue(),
            size: R_NilValue(),
            replace: R_NilValue(),
            prob: R_NilValue(),
        };
        let mut positional = 0usize;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match tag_name(TAG(current)).as_deref() {
                Some("x") => parsed.x = value,
                Some("size") => parsed.size = value,
                Some("replace") => parsed.replace = value,
                Some("prob") => parsed.prob = value,
                _ => {
                    match positional {
                        0 => parsed.x = value,
                        1 => parsed.size = value,
                        2 => parsed.replace = value,
                        3 => parsed.prob = value,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }
        parsed
    }
}

unsafe fn tag_name(tag: SEXP) -> Option<String> {
    unsafe {
        if tag.is_null() || tag == R_NilValue() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
            return None;
        }
        let printname = PRINTNAME(tag);
        if printname.is_null() || printname == R_NilValue() {
            return None;
        }
        let ptr = CHAR(printname);
        if ptr.is_null() {
            return None;
        }
        Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

pub(crate) unsafe fn sample_int_values(
    n: i64,
    size_arg: SEXP,
    replace_arg: SEXP,
    prob_arg: SEXP,
) -> SEXP {
    unsafe {
        let size = parse_n(size_arg, n as c_int);
        let replace = parse_replace(replace_arg);
        if n <= 0 || size <= 0 {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let indices = if is_present(prob_arg) {
            let weights = parse_probability_weights(prob_arg, n as R_xlen_t);
            weighted_sample_indices(&weights, size, replace)
        } else {
            sample_indices(n as R_xlen_t, size, replace)
        };
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, size as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);
        for (out_idx, source_idx) in indices.into_iter().enumerate() {
            *dst.add(out_idx) = source_idx as c_int + 1;
        }
        result
    }
}

fn sample_indices(population_len: R_xlen_t, size: R_xlen_t, replace: bool) -> Vec<usize> {
    if population_len <= 0 || size <= 0 {
        return Vec::new();
    }
    if !replace && size > population_len {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "cannot take a sample larger than the population when 'replace = FALSE'"
                .to_string(),
        });
    }

    if replace {
        let mut indices = Vec::with_capacity(size as usize);
        for _ in 0..size {
            let u = crate::rng::unif_rand();
            let idx = ((u * population_len as f64) as usize).min(population_len as usize - 1);
            indices.push(idx);
        }
        return indices;
    }

    let mut pool: Vec<usize> = (0..population_len as usize).collect();
    for i in 0..size as usize {
        let remaining = pool.len() - i;
        let u = crate::rng::unif_rand();
        let j = i + ((u * remaining as f64) as usize).min(remaining - 1);
        pool.swap(i, j);
    }
    pool.truncate(size as usize);
    pool
}

fn weighted_sample_indices(weights: &[f64], size: R_xlen_t, replace: bool) -> Vec<usize> {
    if weights.is_empty() || size <= 0 {
        return Vec::new();
    }

    let positive = weights.iter().filter(|weight| **weight > 0.0).count();
    if positive == 0 {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "too few positive probabilities".to_string(),
        });
    }
    if !replace && size as usize > positive {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "too few positive probabilities".to_string(),
        });
    }

    let mut active = weights.to_vec();
    let mut indices = Vec::with_capacity(size as usize);
    for _ in 0..size {
        let total: f64 = active.iter().sum();
        if total <= 0.0 {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "too few positive probabilities".to_string(),
            });
        }

        let mut threshold = crate::rng::unif_rand() * total;
        let mut chosen = active.len() - 1;
        for (idx, weight) in active.iter().copied().enumerate() {
            if weight <= 0.0 {
                continue;
            }
            if threshold <= weight {
                chosen = idx;
                break;
            }
            threshold -= weight;
        }

        indices.push(chosen);
        if !replace {
            active[chosen] = 0.0;
        }
    }
    indices
}

fn parse_probability_weights(prob: SEXP, expected_len: R_xlen_t) -> Vec<f64> {
    unsafe {
        if XLENGTH(prob) != expected_len {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "incorrect number of probabilities".to_string(),
            });
        }

        let mut weights = Vec::with_capacity(expected_len as usize);
        for idx in 0..expected_len as usize {
            let weight = match TYPEOF(prob) {
                ty if ty == SEXPTYPE::REALSXP => *REAL(prob).add(idx),
                ty if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP => {
                    let value = *INTEGER(prob).add(idx);
                    if value == crate::sexp::ffi::NA_INTEGER {
                        f64::NAN
                    } else {
                        value as f64
                    }
                }
                _ => {
                    std::panic::panic_any(crate::sexp::context::RError {
                        message: "invalid probability vector".to_string(),
                    });
                }
            };
            if !weight.is_finite() || weight < 0.0 {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "invalid probability vector".to_string(),
                });
            }
            weights.push(weight);
        }
        weights
    }
}

unsafe fn sample_vector_by_indices(x: SEXP, indices: &[usize]) -> SEXP {
    unsafe {
        let result_type = TYPEOF(x);
        let result = Rf_allocVector3(result_type, indices.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        match result_type {
            ty if ty == SEXPTYPE::LGLSXP => {
                for (out_idx, source_idx) in indices.iter().copied().enumerate() {
                    *LOGICAL(result).add(out_idx) = *LOGICAL(x).add(source_idx);
                }
            }
            ty if ty == SEXPTYPE::INTSXP => {
                for (out_idx, source_idx) in indices.iter().copied().enumerate() {
                    *INTEGER(result).add(out_idx) = *INTEGER(x).add(source_idx);
                }
            }
            ty if ty == SEXPTYPE::REALSXP => {
                for (out_idx, source_idx) in indices.iter().copied().enumerate() {
                    *REAL(result).add(out_idx) = *REAL(x).add(source_idx);
                }
            }
            ty if ty == SEXPTYPE::STRSXP => {
                for (out_idx, source_idx) in indices.iter().copied().enumerate() {
                    SET_STRING_ELT(
                        result,
                        out_idx as R_xlen_t,
                        STRING_ELT(x, source_idx as R_xlen_t),
                    );
                }
            }
            ty if ty == SEXPTYPE::VECSXP => {
                for (out_idx, source_idx) in indices.iter().copied().enumerate() {
                    SET_VECTOR_ELT(
                        result,
                        out_idx as R_xlen_t,
                        VECTOR_ELT(x, source_idx as R_xlen_t),
                    );
                }
            }
            _ => {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: format!("unsupported sample() input type '{}'", result_type),
                });
            }
        }
        set_sampled_names(result, x, indices);
        result
    }
}

unsafe fn set_sampled_names(result: SEXP, source: SEXP, indices: &[usize]) {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(source, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null()
            || names == R_NilValue()
            || TYPEOF(names) != SEXPTYPE::STRSXP
            || XLENGTH(names) < XLENGTH(source)
        {
            return;
        }
        let result_names = Rf_allocVector3(SEXPTYPE::STRSXP, indices.len() as R_xlen_t);
        if result_names.is_null() {
            return;
        }
        let _names_guard = protect(result_names);
        for (out_idx, source_idx) in indices.iter().copied().enumerate() {
            SET_STRING_ELT(
                result_names,
                out_idx as R_xlen_t,
                STRING_ELT(names, source_idx as R_xlen_t),
            );
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            result_names,
        );
    }
}

fn parse_replace(arg: SEXP) -> bool {
    unsafe {
        if is_missing_or_null(arg) {
            return false;
        }
        match TYPEOF(arg) {
            ty if ty == SEXPTYPE::LGLSXP => {
                *LOGICAL(arg) != 0 && *LOGICAL(arg) != crate::sexp::ffi::NA_INTEGER
            }
            ty if ty == SEXPTYPE::INTSXP => {
                *INTEGER(arg) != 0 && *INTEGER(arg) != crate::sexp::ffi::NA_INTEGER
            }
            ty if ty == SEXPTYPE::REALSXP => {
                let value = *REAL(arg);
                value != 0.0 && !value.is_nan()
            }
            _ => false,
        }
    }
}

fn sample_int_shortcut_n(x: SEXP) -> Option<i64> {
    unsafe {
        if XLENGTH(x) != 1 {
            return None;
        }
        let value = match TYPEOF(x) {
            ty if ty == SEXPTYPE::INTSXP => {
                let value = *INTEGER(x);
                if value == crate::sexp::ffi::NA_INTEGER {
                    return None;
                }
                value as f64
            }
            ty if ty == SEXPTYPE::REALSXP => *REAL(x),
            _ => return None,
        };
        if value.is_finite() && value >= 1.0 {
            Some(value.floor() as i64)
        } else {
            None
        }
    }
}

fn is_missing_or_null(value: SEXP) -> bool {
    unsafe {
        value.is_null() || value == R_NilValue() || value == crate::sexp::globals::R_UnboundValue()
    }
}

fn is_present(value: SEXP) -> bool {
    !is_missing_or_null(value)
}

// ---------------------------------------------------------------------------
// Register RNG builtins
// ---------------------------------------------------------------------------

/// Register RNG builtins in the base environment.
pub unsafe fn register_rng_builtins(env: SEXP) {
    unsafe {
        use crate::sexp::accessors::SET_FRAME;
        use crate::sexp::constructors::Rf_cons;

        let rng_fns = [
            "set.seed", "RNGkind", "runif", "rnorm", "rpois", "rexp", "sample",
        ];

        let frame = (*env).data.envsxp.frame;
        let mut chain = frame;
        for name in rng_fns {
            let prim = crate::eval::primitive::make_primitive_binding(name, SEXPTYPE::BUILTINSXP);
            let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            let cell = Rf_cons(prim, chain);
            (*cell).data.listsxp.tagval = sym;
            chain = cell;
        }
        SET_FRAME(env, chain);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse the 'n' argument (first arg to r* functions).
fn parse_n(arg: SEXP, default: c_int) -> R_xlen_t {
    unsafe {
        if arg.is_null() || arg == R_NilValue() {
            return default as R_xlen_t;
        }
        let t = TYPEOF(arg);
        if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(arg);
            if v == crate::sexp::ffi::NA_INTEGER || v < 0 {
                return default as R_xlen_t;
            }
            v as R_xlen_t
        } else if t == SEXPTYPE::REALSXP {
            let v = *REAL(arg);
            if v.is_nan() || v < 0.0 {
                return default as R_xlen_t;
            }
            v as R_xlen_t
        } else {
            default as R_xlen_t
        }
    }
}

/// Parse a scalar double argument with a default.
fn parse_double_scalar(arg: SEXP, default: f64) -> f64 {
    unsafe {
        if arg.is_null() || arg == R_NilValue() {
            return default;
        }
        let t = TYPEOF(arg);
        if t == SEXPTYPE::REALSXP {
            *REAL(arg)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(arg);
            if v == crate::sexp::ffi::NA_INTEGER {
                NA_REAL
            } else {
                v as f64
            }
        } else {
            default
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::RSession;
    use crate::sexp::constructors::Rf_cons;

    #[test]
    fn test_runif_returns_vector() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let n = Rf_ScalarInteger(10);
            let min = Rf_ScalarReal(0.0);
            let max = Rf_ScalarReal(1.0);
            let nil = R_NilValue();

            // Build args: n, min, max
            let args = Rf_cons(n, Rf_cons(min, Rf_cons(max, nil)));
            let result = do_runif(R_NilValue(), R_NilValue(), args, R_NilValue());

            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert_eq!(LENGTH(result), 10);

            // All values should be in [0, 1)
            let data = REAL(result);
            for i in 0..10 {
                let v = *data.add(i);
                assert!(v >= 0.0 && v < 1.0, "runif value {} out of range", v);
            }
        });
    }

    #[test]
    fn test_rnorm_returns_vector() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let n = Rf_ScalarInteger(5);
            let mean = Rf_ScalarReal(0.0);
            let sd = Rf_ScalarReal(1.0);
            let nil = R_NilValue();

            let args = Rf_cons(n, Rf_cons(mean, Rf_cons(sd, nil)));
            let result = do_rnorm(R_NilValue(), R_NilValue(), args, R_NilValue());

            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert_eq!(LENGTH(result), 5);
        });
    }

    #[test]
    fn test_set_seed() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let seed = Rf_ScalarInteger(42);
            let nil = R_NilValue();
            let args = Rf_cons(seed, Rf_cons(nil, nil));

            let result = do_set_seed(R_NilValue(), R_NilValue(), args, R_NilValue());
            assert_eq!(result, R_NilValue());
        });
    }

    #[test]
    fn test_runif_seeded_reproducible() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let nil = R_NilValue();
            let seed_args = Rf_cons(Rf_ScalarInteger(123), Rf_cons(nil, nil));
            do_set_seed(R_NilValue(), R_NilValue(), seed_args, R_NilValue());

            let runif_args = Rf_cons(
                Rf_ScalarInteger(3),
                Rf_cons(Rf_ScalarReal(0.0), Rf_cons(Rf_ScalarReal(1.0), nil)),
            );
            let r1 = do_runif(R_NilValue(), R_NilValue(), runif_args, R_NilValue());

            // Reset seed
            let seed_args = Rf_cons(Rf_ScalarInteger(123), Rf_cons(nil, nil));
            do_set_seed(R_NilValue(), R_NilValue(), seed_args, R_NilValue());

            let runif_args = Rf_cons(
                Rf_ScalarInteger(3),
                Rf_cons(Rf_ScalarReal(0.0), Rf_cons(Rf_ScalarReal(1.0), nil)),
            );
            let r2 = do_runif(R_NilValue(), R_NilValue(), runif_args, R_NilValue());

            // Same seed should produce same sequence
            let d1 = REAL(r1);
            let d2 = REAL(r2);
            for i in 0..3 {
                assert_eq!(*d1.add(i), *d2.add(i), "seeded runif not reproducible");
            }
        });
    }

    #[test]
    fn test_rng_kind_is_session_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        left.with_protected(|| {
            set_rng_kind(2);
            assert_eq!(get_rng_kind(), 2);
        });

        right.with_protected(|| {
            assert_eq!(get_rng_kind(), 0);
            set_rng_kind(1);
            assert_eq!(get_rng_kind(), 1);
        });

        left.with_protected(|| {
            assert_eq!(get_rng_kind(), 2);
        });
    }
}
