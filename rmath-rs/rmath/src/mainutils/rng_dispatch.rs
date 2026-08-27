//! R-level `sample()` dispatch — ports R's src/main/random.c sample layer.
//!
//! The RNG surface itself (`set.seed`, `RNGkind`, `runif`, `rnorm`, the
//! `r*` distribution samplers) lives in `mainutils::random` and
//! `library::stats::random`; this module keeps the R-level `sample(x, ...)`
//! wrapper and the `sample.int` internals shared with `essentials`.

use std::ffi::CStr;
use std::os::raw::c_int;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    CAR, CDR, CHAR, INTEGER, LENGTH, LOGICAL, PRINTNAME, REAL, SET_STRING_ELT, SET_VECTOR_ELT,
    STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector3};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

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

/// Parse the 'size' argument of sample()/sample.int(): NULL/missing means
/// "the full population length".
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
