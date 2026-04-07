// F distribution: df, pf, qf, rf
// Ported from df.c, pf.c, qf.c, rf.c

use crate::constants::*;
use crate::dpq::*;
use crate::error::*;
use libm::*;

// Constants
const M_LN_2PI: f64 = 1.837877066409345483560659472811; // log(2*pi)
const DBL_MAX: f64 = 1.7976931348623157e+308;

// ---- dbinom_raw (from dbinom.c) ----
// Used by df(). This is a faithful port of dbinom_raw from R's nmath/dbinom.c.
// It does NOT check that inputs x and n are integers.

fn pow1p(x: f64, y: f64) -> f64 {
    if isnan(y) {
        return if x == 0.0 { 1.0 } else { y };
    }
    if 0.0 <= y && y == trunc(y) && y <= 4.0 {
        match y as i32 {
            0 => return 1.0,
            1 => return x + 1.0,
            2 => return x * (x + 2.0) + 1.0,
            3 => return x * (x * (x + 3.0) + 3.0) + 1.0,
            4 => return x * (x * (x * (x + 4.0) + 6.0) + 4.0) + 1.0,
            _ => {}
        }
    }
    // volatile pattern from C: prevent compiler from optimizing away
    let xp1 = x + 1.0;
    let x_ = xp1 - 1.0;
    if x_ == x || fabs(x) > 0.5 || isnan(x) {
        pow(xp1, y)
    } else {
        exp(y * log1p(x))
    }
}

fn dbinom_raw(x: f64, n: f64, p: f64, q: f64, give_log: bool) -> f64 {
    if p == 0.0 {
        return if x == 0.0 {
            r_d__1(give_log)
        } else {
            r_d__0(give_log)
        };
    }
    if q == 0.0 {
        return if x == n {
            r_d__1(give_log)
        } else {
            r_d__0(give_log)
        };
    }

    if x == 0.0 {
        if n == 0.0 {
            return r_d__1(give_log);
        }
        if p > q {
            return if give_log { n * log(q) } else { pow(q, n) };
        } else {
            return if give_log {
                n * log1p(-p)
            } else {
                pow1p(-p, n)
            };
        }
    }
    if x == n {
        if p > q {
            return if give_log {
                n * log1p(-q)
            } else {
                pow1p(-q, n)
            };
        } else {
            return if give_log { n * log(p) } else { pow(p, n) };
        }
    }
    if x < 0.0 || x > n {
        return r_d__0(give_log);
    }

    if !r_finite(n) {
        if r_finite(x) {
            return r_d__0(give_log);
        } else {
            // n = DBL_MAX helps extreme dnbinom() cases
            let n = DBL_MAX;
            let lc = crate::special::stirlerr::stirlerr(n)
                - crate::special::stirlerr::stirlerr(x)
                - crate::special::stirlerr::stirlerr(n - x)
                - crate::special::bd0::bd0(x, n * p)
                - crate::special::bd0::bd0(n - x, n * q);
            let lf = M_LN_2PI + log(x) + log1p(-x / n);
            return r_d_exp(lc - 0.5 * lf, give_log);
        }
    }

    let lc = crate::special::stirlerr::stirlerr(n)
        - crate::special::stirlerr::stirlerr(x)
        - crate::special::stirlerr::stirlerr(n - x)
        - crate::special::bd0::bd0(x, n * p)
        - crate::special::bd0::bd0(n - x, n * q);

    let lf = M_LN_2PI + log(x) + log1p(-x / n);

    r_d_exp(lc - 0.5 * lf, give_log)
}

// ---- df ----

#[must_use]
pub fn df_inner(x: f64, m: f64, n: f64, give_log: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(m) || isnan(n) {
        return x + m + n;
    }
    if m <= 0.0 || n <= 0.0 {
        return ml_warn_return_nan();
    }
    if x < 0.0 {
        return r_d__0(give_log);
    }
    if x == 0.0 {
        return if m > 2.0 {
            r_d__0(give_log)
        } else if m == 2.0 {
            r_d__1(give_log)
        } else {
            ML_POSINF
        };
    }
    if !r_finite(m) && !r_finite(n) {
        // both +Inf
        if x == 1.0 {
            return ML_POSINF;
        } else {
            return r_d__0(give_log);
        }
    }
    if !r_finite(n) {
        // must be +Inf by now
        return crate::dist::gamma::dgamma_inner(x, m / 2.0, 2.0 / m, give_log);
    }
    if m > 1e14 {
        // includes +Inf: code below is inaccurate there
        let dens = crate::dist::gamma::dgamma_inner(1.0 / x, n / 2.0, 2.0 / n, give_log);
        return if give_log {
            dens - 2.0 * log(x)
        } else {
            dens / (x * x)
        };
    }

    let mut f_val = 1.0 / (n + x * m);
    let q_val = n * f_val;
    let p_val = x * m * f_val;

    let dens = if m >= 2.0 {
        f_val = m * q_val / 2.0;
        dbinom_raw((m - 2.0) / 2.0, (m + n - 2.0) / 2.0, p_val, q_val, give_log)
    } else {
        f_val = m * m * q_val / (2.0 * p_val * (m + n));
        dbinom_raw(m / 2.0, (m + n) / 2.0, p_val, q_val, give_log)
    };
    if give_log {
        log(f_val) + dens
    } else {
        f_val * dens
    }
}

// ---- pf ----

#[must_use]
pub fn pf_inner(x: f64, df1: f64, df2: f64, lower_tail: bool, log_p: bool) -> f64 {
    let mut x = x;

    // IEEE_754
    if isnan(x) || isnan(df1) || isnan(df2) {
        return x + df2 + df1;
    }
    if df1 <= 0.0 || df2 <= 0.0 {
        return ml_warn_return_nan();
    }

    // R_P_bounds_01(x, 0., ML_POSINF);
    if x < 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    if x == 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    if !r_finite(x) {
        return r_dt_1(lower_tail, log_p);
    }

    /* move to pchisq for very large values - was 'df1 > 4e5' in 2.0.x,
    now only needed for df1 = Inf or df2 = Inf {since pbeta(0,*)=0} : */
    if df2 == ML_POSINF {
        if df1 == ML_POSINF {
            if x < 1.0 {
                return r_dt_0(lower_tail, log_p);
            }
            if x == 1.0 {
                return r_d_half(log_p);
            }
            if x > 1.0 {
                return r_dt_1(lower_tail, log_p);
            }
        }
        return crate::dist::chisq::pchisq_inner(x * df1, df1, lower_tail, log_p);
    }

    if df1 == ML_POSINF {
        /* was "fudge" 'df1 > 4e5' in 2.0.x */
        return crate::dist::chisq::pchisq_inner(df2 / x, df2, !lower_tail, log_p);
    }

    /* Avoid squeezing pbeta's first parameter against 1 : */
    if df1 * x > df2 {
        x = crate::dist::beta::pbeta_inner(
            df2 / (df2 + df1 * x),
            df2 / 2.0,
            df1 / 2.0,
            !lower_tail,
            log_p,
        );
    } else {
        x = crate::dist::beta::pbeta_inner(
            df1 * x / (df2 + df1 * x),
            df1 / 2.0,
            df2 / 2.0,
            lower_tail,
            log_p,
        );
    }

    if r_finite(x) { x } else { ML_NAN }
}

// ---- qf ----

#[must_use]
pub fn qf_inner(p: f64, df1: f64, df2: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(p) || isnan(df1) || isnan(df2) {
        return p + df1 + df2;
    }
    if df1 <= 0.0 || df2 <= 0.0 {
        return ml_warn_return_nan();
    }

    // R_Q_P01_boundaries(p, 0, ML_POSINF);
    if log_p {
        if p > 0.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { 0.0 } else { ML_POSINF };
        }
        if p == ML_NEGINF {
            return if lower_tail { ML_POSINF } else { 0.0 };
        }
    } else {
        if p < 0.0 || p > 1.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { 0.0 } else { ML_POSINF };
        }
        if p == 1.0 {
            return if lower_tail { ML_POSINF } else { 0.0 };
        }
    }

    /* fudge the extreme DF cases -- qbeta doesn't do this well.
    But we still need to fudge the infinite ones. */

    if df1 <= df2 && df2 > 4e5 {
        if !r_finite(df1) {
            /* df1 == df2 == Inf : */
            return 1.0;
        }
        /* else value for df2 == Inf : */
        return crate::dist::chisq::qchisq_inner(p, df1, lower_tail, log_p) / df1;
    } else if df1 > 4e5 {
        /* and so df2 < df1 -- return value for df1 == Inf */
        return df2 / crate::dist::chisq::qchisq_inner(p, df2, !lower_tail, log_p);
    }

    // FIXME: (1/qb - 1) = (1 - qb)/qb; if we know qb ~= 1, should use other tail
    let result =
        (1.0 / crate::dist::beta::qbeta_inner(p, df2 / 2.0, df1 / 2.0, !lower_tail, log_p) - 1.0)
            * (df2 / df1);
    if r_finite(result) { result } else { ML_NAN }
}

// ---- rf ----

#[must_use]
pub fn rf_inner(n1: f64, n2: f64) -> f64 {
    if isnan(n1) || isnan(n2) || n1 <= 0.0 || n2 <= 0.0 {
        return ml_warn_return_nan();
    }

    let v1 = if r_finite(n1) {
        crate::dist::chisq::rchisq_inner(n1) / n1
    } else {
        1.0
    };
    let v2 = if r_finite(n2) {
        crate::dist::chisq::rchisq_inner(n2) / n2
    } else {
        1.0
    };
    v1 / v2
}

// ---- FFI shims ----

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn Rf_df(x: f64, m: f64, n: f64, give_log: i32) -> f64 {
    df_inner(x, m, n, give_log != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn df(x: f64, m: f64, n: f64, give_log: i32) -> f64 {
    df_inner(x, m, n, give_log != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn Rf_pf(x: f64, df1: f64, df2: f64, lower_tail: i32, log_p: i32) -> f64 {
    pf_inner(x, df1, df2, lower_tail != 0, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn pf(x: f64, df1: f64, df2: f64, lower_tail: i32, log_p: i32) -> f64 {
    pf_inner(x, df1, df2, lower_tail != 0, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn Rf_qf(p: f64, df1: f64, df2: f64, lower_tail: i32, log_p: i32) -> f64 {
    qf_inner(p, df1, df2, lower_tail != 0, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn qf(p: f64, df1: f64, df2: f64, lower_tail: i32, log_p: i32) -> f64 {
    qf_inner(p, df1, df2, lower_tail != 0, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn Rf_rf(n1: f64, n2: f64) -> f64 {
    rf_inner(n1, n2)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn rf(n1: f64, n2: f64) -> f64 {
    rf_inner(n1, n2)
}
