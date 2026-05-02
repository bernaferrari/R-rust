/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/axis_scales.c
 *
 *  Axis tick mark creation and axis parameter computation.
 */

use std::os::raw::c_int;

use crate::main::coerce::{asInteger, asLogical, coerceVector};
use crate::sexp::accessors::{LENGTH, REAL, SET_STRING_ELT, SET_VECTOR_ELT};
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector, Rf_mkChar};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::protect;

/// R_CreateAtVector - create an axis tick vector.
pub unsafe fn R_CreateAtVector(axp: SEXP, usr: SEXP, nint: SEXP, is_log: SEXP) -> SEXP {
    unsafe {
        let nint = asInteger(nint).max(1);
        let logflag = asLogical(is_log) == 1;
        let axp = coerceVector(axp, SEXPTYPE::REALSXP.into());
        let usr = coerceVector(usr, SEXPTYPE::REALSXP.into());
        if LENGTH(axp) != 3 {
            axis_error("'axp' must be numeric of length 3");
        }
        if LENGTH(usr) != 2 {
            axis_error("'usr' must be numeric of length 2");
        }
        let axp = [*REAL(axp), *REAL(axp).add(1), *REAL(axp).add(2)];
        let usr = [*REAL(usr), *REAL(usr).add(1)];
        let ticks = create_at_vector(axp, usr, nint, logflag);
        real_vector(&ticks)
    }
}

/// R_GAxisPars - compute axis parameters (axp, n) from user range.
pub unsafe fn R_GAxisPars(usr: SEXP, is_log: SEXP, nintLog: SEXP) -> SEXP {
    unsafe {
        let usr = coerceVector(usr, SEXPTYPE::REALSXP.into());
        if LENGTH(usr) != 2 {
            axis_error("'usr' must be numeric of length 2");
        }
        let mut min = *REAL(usr);
        let mut max = *REAL(usr).add(1);
        let mut n = asInteger(nintLog).max(1);
        let logflag = asLogical(is_log) == 1;
        g_axis_pars(&mut min, &mut max, &mut n, logflag);

        let ans = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _ans_guard = protect(ans);
        let axp = Rf_allocVector(SEXPTYPE::REALSXP, 2);
        let _axp_guard = protect(axp);
        *REAL(axp) = min;
        *REAL(axp).add(1) = max;
        SET_VECTOR_ELT(ans, 0, axp);
        SET_VECTOR_ELT(ans, 1, Rf_ScalarInteger(n));

        let names = Rf_allocVector(SEXPTYPE::STRSXP, 2);
        let _names_guard = protect(names);
        SET_STRING_ELT(names, 0, Rf_mkChar(c"axp".as_ptr()));
        SET_STRING_ELT(names, 1, Rf_mkChar(c"n".as_ptr()));
        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), names);
        ans
    }
}

fn axis_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
}

unsafe fn real_vector(values: &[f64]) -> SEXP {
    unsafe {
        let result = Rf_allocVector(SEXPTYPE::REALSXP, values.len() as c_int);
        let _guard = protect(result);
        for (index, value) in values.iter().copied().enumerate() {
            *REAL(result).add(index) = value;
        }
        result
    }
}

fn create_at_vector(axp: [f64; 3], usr: [f64; 2], nint: c_int, logflag: bool) -> Vec<f64> {
    if !logflag || axp[2] < 0.0 {
        return create_linear_at_vector(axp);
    }
    create_log_at_vector(axp, usr, nint, axp[2] as c_int)
}

fn create_linear_at_vector(axp: [f64; 3]) -> Vec<f64> {
    const SMALL_F: f64 = 100.0;
    let n = (axp[2].abs() + 0.25) as usize;
    let dn = n.max(1) as f64;
    let rng = axp[1] - axp[0];
    let small = if rng.is_finite() {
        rng.abs() / SMALL_F / dn
    } else {
        (axp[1] / dn - axp[0] / dn).abs() / SMALL_F
    };
    (0..=n)
        .map(|i| {
            let value = axp[0] + (i as f64 / dn) * rng;
            if value.abs() < small { 0.0 } else { value }
        })
        .collect()
}

fn create_log_at_vector(mut axp: [f64; 3], usr: [f64; 2], nint: c_int, style: c_int) -> Vec<f64> {
    if axp[0] <= 0.0 || axp[1] <= 0.0 {
        axis_error("log-axis tick limits must be positive");
    }
    let reversed = usr[0] > usr[1] && axp[0] > axp[1];
    if reversed {
        axp.swap(0, 1);
    }
    let lower = usr[0].min(usr[1]) * (1.0 - 1e-12);
    let upper = usr[0].max(usr[1]) * (1.0 + 1e-12);
    let factors: &[f64] = match style {
        1 => &[1.0],
        2 => &[1.0, 5.0],
        _ => &[1.0, 2.0, 5.0],
    };
    let start = axp[0].log10().floor() as i32 - 1;
    let end = axp[1].log10().ceil() as i32 + 1;
    let mut ticks = Vec::new();
    for exp in start..=end {
        let base = 10_f64.powi(exp);
        for factor in factors {
            let tick = factor * base;
            if tick >= lower && tick <= upper {
                ticks.push(tick);
            }
        }
    }
    ticks.sort_by(f64::total_cmp);
    ticks.dedup_by(|a, b| (*a - *b).abs() <= f64::EPSILON * a.abs().max(b.abs()).max(1.0));
    if ticks.len() > nint.max(1) as usize + 1 && style == 1 {
        let step = (ticks.len() - 1) / nint.max(1) as usize + 1;
        ticks = ticks.into_iter().step_by(step).collect();
    }
    if reversed {
        ticks.reverse();
    }
    ticks
}

fn g_axis_pars(min: &mut f64, max: &mut f64, n: &mut c_int, logflag: bool) {
    let swap = *min > *max;
    if swap {
        std::mem::swap(min, max);
    }
    let min_original = *min;
    let max_original = *max;

    if logflag {
        *max = (*max).min(308.0);
        *min = (*min).max(-307.0);
        *min = 10_f64.powf(*min);
        *max = 10_f64.powf(*max);
        gl_pretty(min, max, n);
    } else {
        unsafe { crate::mainutils::engine::GEPretty(min, max, n) };
    }

    let t = max.abs().max(min.abs());
    let tf = if t > 1.0 {
        (t * f64::EPSILON) * 16.0
    } else {
        (t * 16.0) * f64::EPSILON
    }
    .max(f64::MIN_POSITIVE);
    if (*max - *min).abs() <= tf {
        *min = min_original;
        *max = max_original;
        let eps = 0.005 * (*max - *min);
        *min += eps;
        *max -= eps;
        if logflag {
            *min = 10_f64.powf(*min);
            *max = 10_f64.powf(*max);
        }
        *n = 1;
    }
    if swap {
        std::mem::swap(min, max);
    }
}

fn gl_pretty(ul: &mut f64, uh: &mut f64, n: &mut c_int) {
    let p1 = ul.log10().ceil() as i32;
    let p2 = uh.log10().floor() as i32;
    if p2 <= p1 {
        unsafe { crate::mainutils::engine::GEPretty(ul, uh, n) };
        *n = -*n;
    } else {
        *ul = 10_f64.powi(p1);
        *uh = 10_f64.powi(p2);
        *n = if p2 - p1 <= 2 {
            3
        } else if p2 - p1 <= 3 {
            2
        } else {
            1
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::accessors::{INTEGER, VECTOR_ELT};

    unsafe fn real(values: &[f64]) -> SEXP {
        unsafe { real_vector(values) }
    }

    #[test]
    fn test_create_at_vector_linear() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let axp = real(&[0.0, 10.0, 5.0]);
            let usr = real(&[0.0, 10.0]);
            let out = R_CreateAtVector(axp, usr, Rf_ScalarInteger(5), Rf_ScalarInteger(0));
            assert_eq!(LENGTH(out), 6);
            assert_eq!(*REAL(out), 0.0);
            assert_eq!(*REAL(out).add(5), 10.0);
        }
    }

    #[test]
    fn test_axis_pars_linear_returns_named_shape() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let usr = real(&[0.0, 10.0]);
            let out = R_GAxisPars(usr, Rf_ScalarInteger(0), Rf_ScalarInteger(5));
            assert_eq!(LENGTH(out), 2);
            let axp = VECTOR_ELT(out, 0);
            assert_eq!(LENGTH(axp), 2);
            assert!(*REAL(axp) <= *REAL(axp).add(1));
            assert!(*INTEGER(VECTOR_ELT(out, 1)) >= 1);
        }
    }

    #[test]
    fn test_create_at_vector_log_has_positive_ticks() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let axp = real(&[1.0, 1000.0, 3.0]);
            let usr = real(&[1.0, 1000.0]);
            let out = R_CreateAtVector(axp, usr, Rf_ScalarInteger(5), Rf_ScalarInteger(1));
            assert!(LENGTH(out) > 0);
            for i in 0..LENGTH(out) as usize {
                assert!(*REAL(out).add(i) > 0.0);
            }
        }
    }
}
