// Wilcoxon signed rank distribution: dsignrank, psignrank, qsignrank, rsignrank
// Ported from R's nmath/signrank.c
//
// Original by R Core Team

use crate::constants::*;
use crate::dpq::*;
use crate::error::*;
use crate::rng::*;
#[cfg(not(target_arch = "wasm32"))]
use crate::sexp::instance::with_required_current_instance;
use crate::utils::*;
#[cfg(target_arch = "wasm32")]
use crate::wasm_shim::with_required_current_instance;
use libm::*;
use std::collections::HashMap;
use std::os::raw::{c_double, c_int};

// Constants
const M_LN2: f64 = 0.693147180559945309417232121458;
const DBL_EPSILON: f64 = 2.220446049250313e-16;

// Per-session cached workspace for csignrank.
// w[n] is a Vec<f64> of size (c+1) where c = n*(n+1)/4 (truncated).
// A value of -1.0 means "not yet computed".
fn with_signrank_cache<F, R>(f: F) -> R
where
    F: FnOnce(&mut HashMap<i32, Vec<f64>>) -> R,
{
    with_required_current_instance(|instance| f(&mut instance.signrank_cache))
}

/// csignrank: counts for the signed rank distribution.
/// Returns the number of ways to get statistic = k for n signed ranks.
fn csignrank(k: i32, n: i32) -> f64 {
    let u = n * (n + 1) / 2;
    let c = (u / 2) as i32;

    if k < 0 || k > u {
        return 0.0;
    }
    let mut k = k;
    if k > c {
        k = u - k;
    }

    if n == 1 {
        return 1.0;
    }

    with_signrank_cache(|cache| {
        let entry = cache
            .entry(n)
            .or_insert_with(|| vec![-1.0_f64; (c + 1) as usize]);

        // Check if already computed (w[0] == 1 means initialized)
        if entry[0] == 1.0 {
            return entry[k as usize];
        }

        // Initialize: w[0] = w[1] = 1
        entry[0] = 1.0;
        if entry.len() > 1 {
            entry[1] = 1.0;
        }

        for j in 2..=(n as usize) {
            let end = imin2((j * (j + 1) / 2) as i32, c as i32) as usize;
            let jj = j;
            for i in (jj..=end).rev() {
                entry[i] += entry[i - jj];
            }
        }

        entry[k as usize]
    })
}

// =====================================================================
// dsignrank
// =====================================================================

#[must_use]
pub fn dsignrank_inner(x: f64, n: f64, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(n) {
        return x + n;
    }
    let n = r_forceint(n);
    if n <= 0.0 {
        return ml_warn_return_nan();
    }

    if r_nonint(x) {
        return r_d__0(log_p);
    }
    let x = r_forceint(x);
    if x < 0.0 || x > n * (n + 1.0) / 2.0 {
        return r_d__0(log_p);
    }

    let nn = n as i32;
    let d = r_d_exp(log(csignrank(x as i32, nn)) - n * M_LN2, log_p);

    d
}

// =====================================================================
// psignrank
// =====================================================================

#[must_use]
pub fn psignrank_inner(x: f64, n: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(n) {
        return x + n;
    }
    if !r_finite(n) {
        return ml_warn_return_nan();
    }
    let n = r_forceint(n);
    if n <= 0.0 {
        return ml_warn_return_nan();
    }

    let x = floor(x + 1e-7);
    if x < 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    if x >= n * (n + 1.0) / 2.0 {
        return r_dt_1(lower_tail, log_p);
    }

    let nn = n as i32;
    let f = exp(-n * M_LN2);
    let mut p = 0.0;
    let mut lower_tail = lower_tail;

    if x <= n * (n + 1.0) / 4.0 {
        for i in 0..=(x as i32) {
            p += csignrank(i, nn) * f;
        }
    } else {
        let x_new = (n * (n + 1.0) / 2.0 - x) as i32;
        for i in 0..x_new {
            p += csignrank(i, nn) * f;
        }
        lower_tail = !lower_tail; // p = 1 - p;
    }

    r_dt_val(p, lower_tail, log_p)
}

// =====================================================================
// qsignrank
// =====================================================================

#[must_use]
pub fn qsignrank_inner(x: f64, n: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(n) {
        return x + n;
    }
    if !r_finite(x) || !r_finite(n) {
        return ml_warn_return_nan();
    }

    // R_Q_P01_check(x)
    if log_p {
        if x > 0.0 {
            return ml_warn_return_nan();
        }
        if x == 0.0 {
            return if lower_tail { ML_POSINF } else { 0.0 };
        }
        if x == ML_NEGINF {
            return if lower_tail { 0.0 } else { ML_POSINF };
        }
    } else {
        if x < 0.0 || x > 1.0 {
            return ml_warn_return_nan();
        }
    }

    let n = r_forceint(n);
    if n <= 0.0 {
        return ml_warn_return_nan();
    }

    // R_Q_P01_boundaries(p, 0, n*(n+1)/2)
    let p_is_0 = if log_p {
        x == if lower_tail { ML_NEGINF } else { 0.0 }
    } else {
        x == if lower_tail { 0.0 } else { 1.0 }
    };
    let p_is_1 = if log_p {
        x == if lower_tail { 0.0 } else { ML_NEGINF }
    } else {
        x == if lower_tail { 1.0 } else { 0.0 }
    };

    if p_is_0 {
        return 0.0;
    }
    if p_is_1 {
        return n * (n + 1.0) / 2.0;
    }

    let mut x = x;
    if log_p || !lower_tail {
        x = r_dt_qiv(x, lower_tail, log_p); // lower_tail, non-log "p"
    }

    let nn = n as i32;
    let f = exp(-n * M_LN2);
    let mut p = 0.0;
    let mut q: i32 = 0;

    if x <= 0.5 {
        x -= 10.0 * DBL_EPSILON;
        loop {
            p += csignrank(q, nn) * f;
            if p >= x {
                break;
            }
            q += 1;
        }
    } else {
        x = 1.0 - x + 10.0 * DBL_EPSILON;
        loop {
            p += csignrank(q, nn) * f;
            if p > x {
                q = (n * (n + 1.0) / 2.0 - q as f64) as i32;
                break;
            }
            q += 1;
        }
    }

    q as f64
}

// =====================================================================
// rsignrank
// =====================================================================

#[must_use]
pub fn rsignrank_inner(n: f64) -> f64 {
    // IEEE_754
    if isnan(n) {
        return n;
    }
    let n = r_forceint(n);
    if n < 0.0 {
        return ml_warn_return_nan();
    }

    if n == 0.0 {
        return 0.0;
    }

    let mut r = 0.0;
    let k = n as i32;
    let mut i = 0;
    while i < k {
        i += 1;
        r += i as f64 * floor(unif_rand() + 0.5);
    }
    r
}

// =====================================================================
// FFI shims
// =====================================================================

#[must_use]
pub fn Rf_dsignrank(x: c_double, n: c_double, give_log: c_int) -> c_double {
    dsignrank_inner(x, n, give_log != 0)
}

#[must_use]
pub fn dsignrank(x: c_double, n: c_double, give_log: c_int) -> c_double {
    dsignrank_inner(x, n, give_log != 0)
}

pub fn Rf_psignrank(x: c_double, n: c_double, lower_tail: c_int, log_p: c_int) -> c_double {
    psignrank_inner(x, n, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn psignrank(x: c_double, n: c_double, lower_tail: c_int, log_p: c_int) -> c_double {
    psignrank_inner(x, n, lower_tail != 0, log_p != 0)
}

pub fn Rf_qsignrank(p: c_double, n: c_double, lower_tail: c_int, log_p: c_int) -> c_double {
    qsignrank_inner(p, n, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn qsignrank(p: c_double, n: c_double, lower_tail: c_int, log_p: c_int) -> c_double {
    qsignrank_inner(p, n, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_rsignrank(n: c_double) -> c_double {
    rsignrank_inner(n)
}

#[must_use]
pub fn rsignrank(n: c_double) -> c_double {
    rsignrank_inner(n)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::sexp::RSession;

    #[test]
    fn signrank_cache_is_session_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        let left_value = left.with_protected(|| csignrank(3, 4));
        right.with_protected(|| {
            with_signrank_cache(|cache| assert!(!cache.contains_key(&4)));
            assert_eq!(csignrank(3, 4), left_value);
        });

        left.with_protected(|| {
            with_signrank_cache(|cache| assert!(cache.contains_key(&4)));
        });
    }
}
