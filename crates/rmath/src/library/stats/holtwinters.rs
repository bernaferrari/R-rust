//! Holt-Winters filtering algorithm.
//! Port of r-source/src/library/stats/src/HoltWinters.c

use crate::sexp::ffi::SEXP;
use core::ffi::c_int;

pub(crate) mod kernel;

fn hw_golden_min(lo: f64, hi: f64, steps: usize, mut f: impl FnMut(f64) -> f64) -> f64 {
    let gr = 0.5 * (5.0_f64.sqrt() - 1.0);
    let mut lo = lo;
    let mut hi = hi;
    let mut c = hi - gr * (hi - lo);
    let mut d = lo + gr * (hi - lo);
    let mut fc = f(c);
    let mut fd = f(d);
    for _ in 0..steps {
        if fc < fd {
            hi = d;
            d = c;
            fd = fc;
            c = hi - gr * (hi - lo);
            fc = f(c);
        } else {
            lo = c;
            c = d;
            fc = fd;
            d = lo + gr * (hi - lo);
            fd = f(d);
        }
    }
    0.5 * (lo + hi)
}

fn hw_additive_start(x: &[f64], period: usize) -> (f64, f64, Vec<f64>) {
    let wind = 2 * period;
    if period < 2 || x.len() < wind {
        return (
            x.first().copied().unwrap_or(0.0),
            0.0,
            vec![0.0; period.max(1)],
        );
    }
    let flen = period + 1;
    let mut filt = vec![1.0; flen];
    filt[0] = 0.5;
    filt[flen - 1] = 0.5;
    let p = period as f64;
    for v in &mut filt {
        *v /= p;
    }
    let off = period / 2;
    let mut trend = vec![f64::NAN; wind];
    for i in off..(wind - off) {
        let mut s = 0.0;
        for k in 0..flen {
            let j = i as isize - off as isize + k as isize;
            if j >= 0 && (j as usize) < wind {
                s += filt[k] * x[j as usize];
            }
        }
        trend[i] = s;
    }
    let mut fig = vec![0.0; period];
    let mut cnt = vec![0.0; period];
    for i in 0..wind {
        if trend[i].is_finite() {
            fig[i % period] += x[i] - trend[i];
            cnt[i % period] += 1.0;
        }
    }
    for i in 0..period {
        if cnt[i] > 0.0 {
            fig[i] /= cnt[i];
        }
    }
    let mean = fig.iter().sum::<f64>() / period as f64;
    for v in &mut fig {
        *v -= mean;
    }
    let mut nobs = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut sxx = 0.0;
    let mut sxy = 0.0;
    for i in 0..wind {
        if trend[i].is_finite() {
            nobs += 1.0;
            let t = nobs;
            sx += t;
            sy += trend[i];
            sxx += t * t;
            sxy += t * trend[i];
        }
    }
    let det = nobs * sxx - sx * sx;
    if det.abs() < 1e-12 || nobs < 2.0 {
        return (sy / nobs.max(1.0), 0.0, fig);
    }
    let b = (nobs * sxy - sx * sy) / det;
    let a = (sy - b * sx) / nobs;
    (a, b, fig)
}

fn hw_additive_sse(
    x: &[f64],
    alpha: f64,
    beta: f64,
    gamma: f64,
    a0: f64,
    b0: f64,
    s0: &[f64],
    start_time: usize,
    period: usize,
) -> f64 {
    let n = x.len();
    if start_time == 0 || start_time > n || period == 0 {
        return f64::INFINITY;
    }
    let nfit = n - start_time + 1;
    let mut level = vec![0.0; nfit + 1];
    let mut trend = vec![0.0; nfit + 1];
    let mut season = vec![0.0; n + period];
    let mut sse = 0.0;
    let parameters = kernel::Parameters {
        alpha,
        beta,
        gamma,
        start_time,
        period,
        additive: true,
        trend: true,
        seasonal: true,
    };
    if kernel::filter(
        x,
        parameters,
        kernel::Initial {
            level: a0,
            trend: b0,
            season: s0,
        },
        kernel::Output {
            sse: std::slice::from_mut(&mut sse),
            level: &mut level,
            trend: &mut trend,
            season: &mut season,
        },
    )
    .is_err()
    {
        return f64::INFINITY;
    }
    sse
}

/// GNU `HoltWinters` additive, first-period start.
pub unsafe fn do_HoltWinters(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, INTEGER, REAL, SET_VECTOR_ELT, TYPEOF, XLENGTH};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkString};
        use crate::sexp::ffi::{SEXP, SEXPTYPE};
        use crate::sexp::protect::protect;
        use crate::sexp::symbol::Rf_install;
        let x0 = CAR(args);
        let n = XLENGTH(x0) as c_int;
        let tsp = crate::sexp::attrib_core::getAttrib(x0, Rf_install(c"tsp".as_ptr()));
        let period = if !tsp.is_null() && TYPEOF(tsp) == SEXPTYPE::REALSXP && XLENGTH(tsp) >= 3 {
            *REAL(tsp).add(2) as c_int
        } else {
            12
        };
        if n < 2 * period {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "time series has no or less than 2 periods",
            );
        }
        let mut x = vec![0.0f64; n as usize];
        for i in 0..n as usize {
            x[i] = if TYPEOF(x0) == SEXPTYPE::REALSXP {
                *REAL(x0).add(i)
            } else {
                *INTEGER(x0).add(i) as f64
            };
        }
        let start_time = (period + 1) as usize;
        let (a0, b0, s0) = hw_additive_start(&x, period as usize);
        let mut alpha = 0.3;
        let mut beta = 0.1;
        let mut gamma = 0.1;
        for _ in 0..25 {
            alpha = hw_golden_min(0.0, 1.0, 50, |t| {
                hw_additive_sse(&x, t, beta, gamma, a0, b0, &s0, start_time, period as usize)
            });
            beta = hw_golden_min(0.0, 1.0, 50, |t| {
                hw_additive_sse(
                    &x,
                    alpha,
                    t,
                    gamma,
                    a0,
                    b0,
                    &s0,
                    start_time,
                    period as usize,
                )
            });
            gamma = hw_golden_min(0.0, 1.0, 50, |t| {
                hw_additive_sse(&x, alpha, beta, t, a0, b0, &s0, start_time, period as usize)
            });
        }
        if alpha.abs() < 1e-6 {
            alpha = 0.0;
        }
        if beta.abs() < 1e-6 {
            beta = 0.0;
        }
        if gamma.abs() < 1e-6 {
            gamma = 0.0;
        }
        let a = a0;
        let b = b0;
        let s = s0;
        let mut sse = 0.0;
        let nfit = (n - period) as usize;
        let mut level = vec![0.0f64; nfit + 1];
        let mut trend = vec![0.0f64; nfit + 1];
        let mut season = vec![0.0f64; n as usize + period as usize];
        let parameters = kernel::Parameters {
            alpha,
            beta,
            gamma,
            start_time,
            period: period as usize,
            additive: true,
            trend: true,
            seasonal: true,
        };
        if let Err(error) = kernel::filter(
            &x,
            parameters,
            kernel::Initial {
                level: a,
                trend: b,
                season: &s,
            },
            kernel::Output {
                sse: std::slice::from_mut(&mut sse),
                level: &mut level,
                trend: &mut trend,
                season: &mut season,
            },
        ) {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                &error.to_string(),
            );
        }
        let fitted = Rf_allocVector3(SEXPTYPE::REALSXP, (nfit * 4) as i64);
        let _f = protect(fitted);
        for i in 0..nfit {
            let lv = level[i];
            let tr = trend[i];
            let se = season[i];
            *REAL(fitted).add(i) = lv + tr + se;
            *REAL(fitted).add(i + nfit) = lv;
            *REAL(fitted).add(i + 2 * nfit) = tr;
            *REAL(fitted).add(i + 3 * nfit) = se;
        }
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(dim) = nfit as c_int;
        *INTEGER(dim).add(1) = 4;
        crate::sexp::attrib_core::setAttrib(fitted, crate::sexp::attrib_core::R_DimSymbol(), dim);
        let ncoef = 2 + period as i64;
        let coef = Rf_allocVector3(SEXPTYPE::REALSXP, ncoef);
        let _cf = protect(coef);
        *REAL(coef) = a;
        *REAL(coef).add(1) = b;
        for i in 0..period as usize {
            *REAL(coef).add(2 + i) = s[i];
        }
        let mut cnames = vec!["a".to_string(), "b".to_string()];
        for i in 1..=period {
            cnames.push(format!("s{i}"));
        }
        crate::mainutils::essentials::set_string_names(coef, &cnames);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 8);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, fitted);
        SET_VECTOR_ELT(result, 1, crate::sexp::constructors::Rf_ScalarReal(alpha));
        SET_VECTOR_ELT(result, 2, crate::sexp::constructors::Rf_ScalarReal(beta));
        SET_VECTOR_ELT(result, 3, crate::sexp::constructors::Rf_ScalarReal(gamma));
        SET_VECTOR_ELT(result, 4, crate::sexp::constructors::Rf_ScalarReal(sse));
        SET_VECTOR_ELT(result, 5, coef);
        SET_VECTOR_ELT(result, 6, x0);
        SET_VECTOR_ELT(result, 7, Rf_mkString(c"additive".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "fitted".to_string(),
                "alpha".to_string(),
                "beta".to_string(),
                "gamma".to_string(),
                "SSE".to_string(),
                "coefficients".to_string(),
                "x".to_string(),
                "seasonal".to_string(),
            ],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"HoltWinters".as_ptr()),
        );
        result
    }
}
