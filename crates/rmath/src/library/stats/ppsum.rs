//! Partial autocorrelation sum and integration helpers
//! Port of r-source/src/library/stats/src/PPsum.c

use std::os::raw::{c_double, c_int};
use std::slice;

use crate::main::coerce::asInteger;
use crate::mainutils::errors::{Rf_error, Rf_error_cstr};
use crate::sexp::accessors::{LENGTH, REAL};
use crate::sexp::constructors::{Rf_ScalarReal, Rf_allocVector};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::protect;

// `crate::main::coerce::coerceVector` still takes `c_int`; keep that conversion
// local so the rest of this file uses `SEXPTYPE` directly.
fn coerceVector(x: SEXP, sexptype: SEXPTYPE) -> SEXP {
    crate::main::coerce::coerceVector(x, sexptype.into())
}

pub(crate) fn r_pp_sum(u: &[c_double], l: c_int) -> c_double {
    let mut tmp1 = 0.0;
    if l > 0 {
        let lag_denominator = l as c_double + 1.0;
        let max_lag = (l as usize).min(u.len().saturating_sub(1));
        for lag in 1..=max_lag {
            let mut tmp2 = 0.0;
            for j in lag..u.len() {
                tmp2 += u[j] * u[j - lag];
            }
            tmp2 *= 1.0 - lag as c_double / lag_denominator;
            tmp1 += tmp2;
        }
    }
    2.0 * tmp1 / u.len() as c_double
}

fn integrate_values(x: &[c_double], xi: &[c_double], lag: usize, y: &mut [c_double]) {
    y.fill(0.0);
    let prefix = lag.min(xi.len()).min(y.len());
    y[..prefix].copy_from_slice(&xi[..prefix]);

    for (src, dest) in x.iter().zip(lag..lag + x.len()) {
        y[dest] = *src + y[dest - lag];
    }
}

fn error(msg: &'static [u8]) -> ! {
    Rf_error_cstr(crate::mainutils::errors::cstr_until_nul(msg));

    unreachable!("Rf_error returned");
}

fn non_negative_usize(value: c_int, name: &'static [u8]) -> usize {
    if value < 0 {
        error(name);
    }
    value as usize
}

pub fn pp_sum(u: SEXP, sl: SEXP) -> SEXP {
    let u = coerceVector(u, SEXPTYPE::REALSXP);
    let _u_guard = protect(u);
    let n = LENGTH(u);
    let l = asInteger(sl);
    let values_values = crate::library::stats::checked::real_vec(u);
    let values = values_values.as_slice();
    let trm = r_pp_sum(values, l);
    Rf_ScalarReal(trm)
}

pub fn intgrt_vec(x: SEXP, xi: SEXP, slag: SEXP) -> SEXP {
    let x = coerceVector(x, SEXPTYPE::REALSXP);
    let _x_guard = protect(x);
    let xi = coerceVector(xi, SEXPTYPE::REALSXP);
    let _xi_guard = protect(xi);

    let n = LENGTH(x);
    let xi_len = LENGTH(xi);
    let lag = asInteger(slag);
    let lag_usize = non_negative_usize(lag, b"'lag' must be non-negative\0");
    if xi_len < lag {
        error(b"'xi' is shorter than 'lag'\0");
    }
    let output_len = n
        .checked_add(lag)
        .unwrap_or_else(|| error(b"result length is too large\0"));
    let ans = Rf_allocVector(SEXPTYPE::REALSXP, output_len);
    let _ans_guard = protect(ans);

    let x_values_values = crate::library::stats::checked::real_vec(x);
    let x_values = x_values_values.as_slice();
    let xi_values_values = crate::library::stats::checked::real_vec(xi);
    let xi_values = xi_values_values.as_slice();
    let mut y = vec![0.0; n as usize + lag_usize];
    integrate_values(x_values, xi_values, lag_usize, &mut y);
    crate::library::stats::checked::set_reals(ans, &y);

    ans
}
