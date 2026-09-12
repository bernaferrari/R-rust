//! Holt-Winters filtering algorithm.
//! Port of r-source/src/library/stats/src/HoltWinters.c

use core::ffi::{c_double, c_int, c_void};
use crate::sexp::ffi::SEXP;

/// Holt-Winters filtering.
///
/// Port of `HoltWinters` from R's `src/library/stats/src/HoltWinters.c`.
///
/// # Safety
/// All pointer arguments must be valid and point to appropriately sized arrays.
pub unsafe fn HoltWinters(
    x: *mut c_double,
    xl: *mut c_int,
    alpha: *mut c_double,
    beta: *mut c_double,
    gamma: *mut c_double,
    start_time: *mut c_int,
    seasonal: *mut c_int,
    period: *mut c_int,
    dotrend: *mut c_int,
    doseasonal: *mut c_int,
    a: *mut c_double,
    b: *mut c_double,
    s: *mut c_double,
    SSE: *mut c_double,
    level: *mut c_double,
    trend: *mut c_double,
    season: *mut c_double,
) {
    unsafe {
        let mut res: c_double = 0.0;
        let mut xhat: c_double = 0.0;
        let mut stmp: c_double = 0.0;

        let xl_val = *xl;
        let start_time_val = *start_time;
        let seasonal_val = *seasonal;
        let period_val = *period;
        let dotrend_val = *dotrend;
        let doseasonal_val = *doseasonal;

        *level = *a;
        if dotrend_val == 1 {
            *trend = *b;
        }
        if doseasonal_val == 1 && period_val != 0 {
            std::ptr::copy_nonoverlapping(s, season, period_val as usize);
        }

        let mut i = start_time_val - 1;
        while i < xl_val {
            let i0 = i - start_time_val + 2;
            let s0 = i0 + period_val - 1;

            xhat = *level.add((i0 - 1) as usize)
                + if dotrend_val == 1 {
                    *trend.add((i0 - 1) as usize)
                } else {
                    0.0
                };
            stmp = if doseasonal_val == 1 {
                *season.add((s0 - period_val) as usize)
            } else {
                if seasonal_val != 1 { 1.0 } else { 0.0 }
            };
            if seasonal_val == 1 {
                xhat += stmp;
            } else {
                xhat *= stmp;
            }
            res = *x.add(i as usize) - xhat;
            *SSE += res * res;

            if seasonal_val == 1 {
                *level.add(i0 as usize) = *alpha * (*x.add(i as usize) - stmp)
                    + (1.0 - *alpha)
                        * (*level.add((i0 - 1) as usize) + *trend.add((i0 - 1) as usize));
            } else {
                *level.add(i0 as usize) = *alpha * (*x.add(i as usize) / stmp)
                    + (1.0 - *alpha)
                        * (*level.add((i0 - 1) as usize) + *trend.add((i0 - 1) as usize));
            }

            if dotrend_val == 1 {
                *trend.add(i0 as usize) = *beta
                    * (*level.add(i0 as usize) - *level.add((i0 - 1) as usize))
                    + (1.0 - *beta) * *trend.add((i0 - 1) as usize);
            }

            if doseasonal_val == 1 {
                if seasonal_val == 1 {
                    *season.add(s0 as usize) = *gamma
                        * (*x.add(i as usize) - *level.add(i0 as usize))
                        + (1.0 - *gamma) * stmp;
                } else {
                    *season.add(s0 as usize) = *gamma
                        * (*x.add(i as usize) / *level.add(i0 as usize))
                        + (1.0 - *gamma) * stmp;
                }
            }

            i += 1;
        }
    }
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
        let period = if !tsp.is_null()
            && TYPEOF(tsp) == SEXPTYPE::REALSXP
            && XLENGTH(tsp) >= 3
        {
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
        let mut alpha = 1.0;
        let mut beta = 0.0;
        let mut gamma = 0.1;
        let mut start_time = period + 1;
        let mut seasonal = 1;
        let mut dotrend = 1;
        let mut doseasonal = 1;
        let mut a = x[(period - 1) as usize] - (period as f64) / 2.0;
        let mut b = 1.0;
        let mut s = vec![0.0f64; period as usize];
        let mut sse = 0.0;
        let nfit = (n - period) as usize;
        let mut level = vec![0.0f64; nfit + 1];
        let mut trend = vec![0.0f64; nfit + 1];
        let mut season = vec![0.0f64; n as usize + period as usize];
        let mut xl = n;
        let mut per = period;
        HoltWinters(
            x.as_mut_ptr(),
            &mut xl,
            &mut alpha,
            &mut beta,
            &mut gamma,
            &mut start_time,
            &mut seasonal,
            &mut per,
            &mut dotrend,
            &mut doseasonal,
            &mut a,
            &mut b,
            s.as_mut_ptr(),
            &mut sse,
            level.as_mut_ptr(),
            trend.as_mut_ptr(),
            season.as_mut_ptr(),
        );
        let fitted = Rf_allocVector3(SEXPTYPE::REALSXP, (nfit * 4) as i64);
        let _f = protect(fitted);
        for i in 0..nfit {
            let lv = level[i];
            let tr = trend[i];
            let se = 0.0;
            *REAL(fitted).add(i) = lv + tr + se;
            *REAL(fitted).add(i + nfit) = lv;
            *REAL(fitted).add(i + 2 * nfit) = tr;
            *REAL(fitted).add(i + 3 * nfit) = se;
        }
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(dim) = nfit as c_int;
        *INTEGER(dim).add(1) = 4;
        crate::sexp::attrib_core::setAttrib(fitted, crate::sexp::attrib_core::R_DimSymbol(), dim);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, fitted);
        crate::mainutils::essentials::set_string_names(result, &["fitted".to_string()]);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"HoltWinters".as_ptr()),
        );
        result
    }
}

