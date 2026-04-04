// Wilcoxon rank sum distribution: dwilcox, pwilcox, qwilcox, rwilcox
// Ported from R's nmath/wilcox.c
//
// Original by R Core Team, based on AS70 (C) 1974 Royal Statistical Society

use crate::constants::*;
use crate::dpq::*;
use crate::error::*;
use crate::rng::*;
use crate::special::gamma::lgammafn;
use crate::utils::*;
use libm::*;
use std::cell::RefCell;
use std::os::raw::{c_double, c_int};

// Constants
const DBL_EPSILON: f64 = 2.220446049250313e-16;

// Thread-local cached workspace for cwilcox.
// w[i][j] is a Vec<f64> of size (c+1) where c = m*n/2 (when i,j are swapped to i<=j).
// A value of -1.0 means "not yet computed".
// We store a HashMap keyed by (i, j) pairs.
use std::collections::HashMap;
thread_local! {
    static W_CACHE: RefCell<HashMap<(i32, i32), Vec<f64>>> = RefCell::new(HashMap::new());
}

/// Compute log(choose(n, k)) = lgammafn(n+1) - lgammafn(k+1) - lgammafn(n-k+1)
fn lchoose(n: f64, k: f64) -> f64 {
    lgammafn(n + 1.0) - lgammafn(k + 1.0) - lgammafn(n - k + 1.0)
}

/// cwilcox: count the number of choices with statistic = k
/// This counts the number of subsets of size n from {1, ..., m+n}
/// whose Wilcoxon rank sum statistic equals k.
fn cwilcox(k: i32, m: i32, n: i32) -> f64 {
    let u = m * n;
    if k < 0 || k > u {
        return 0.0;
    }
    let mut k = k;
    let c = (u / 2) as i32;
    if k > c {
        k = u - k; // hence k <= floor(u / 2)
    }

    let (i, j) = if m < n { (m, n) } else { (n, m) };
    // hence i <= j

    if j == 0 {
        // and hence i == 0
        return if k == 0 { 1.0 } else { 0.0 };
    }

    // Simplify: if k < j, same count as cwilcox(k, i, k)
    if j > 0 && k < j {
        return cwilcox(k, i, k);
    }

    // Check cache first
    let cached = W_CACHE.with(|cache| {
        let cache = cache.borrow();
        if let Some(entry) = cache.get(&(i, j))
            && entry.len() > k as usize && entry[k as usize] >= 0.0
        {
            return Some(entry[k as usize]);
        }
        None
    });
    if let Some(val) = cached {
        return val;
    }

    // Compute value (recursive calls happen outside the borrow)
    let val = if j == 0 {
        if k == 0 {
            1.0
        } else {
            0.0
        }
    } else {
        cwilcox(k - j, i - 1, j) + cwilcox(k, i, j - 1)
    };

    // Store in cache
    W_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let entry = cache
            .entry((i, j))
            .or_insert_with(|| vec![-1.0_f64; (c + 1) as usize]);
        if (k as usize) < entry.len() {
            entry[k as usize] = val;
        }
    });

    val
}

// =====================================================================
// dwilcox
// =====================================================================

pub fn dwilcox_inner(x: f64, m: f64, n: f64, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(m) || isnan(n) {
        return x + m + n;
    }
    let m = r_forceint(m);
    let n = r_forceint(n);
    if m <= 0.0 || n <= 0.0 {
        return ml_warn_return_nan();
    }

    if r_nonint(x) {
        return r_d__0(log_p);
    }
    let x = r_forceint(x);
    if x < 0.0 || x > m * n {
        return r_d__0(log_p);
    }

    let mm = m as i32;
    let nn = n as i32;
    let xx = x as i32;

    let d = if log_p {
        log(cwilcox(xx, mm, nn)) - lchoose(m + n, n)
    } else {
        cwilcox(xx, mm, nn) / exp(lchoose(m + n, n))
    };

    d
}

// =====================================================================
// pwilcox
// =====================================================================

pub fn pwilcox_inner(q: f64, m: f64, n: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(q) || isnan(m) || isnan(n) {
        return q + m + n;
    }
    if !r_finite(m) || !r_finite(n) {
        return ml_warn_return_nan();
    }
    let m = r_forceint(m);
    let n = r_forceint(n);
    if m <= 0.0 || n <= 0.0 {
        return ml_warn_return_nan();
    }

    let q = floor(q + 1e-7);

    if q < 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    if q >= m * n {
        return r_dt_1(lower_tail, log_p);
    }

    let mm = m as i32;
    let nn = n as i32;

    let denom = exp(lchoose(m + n, n));
    let mut p = 0.0;
    let mut lower_tail = lower_tail;

    // Use summation of probs over the shorter range
    if q <= (m * n / 2.0) {
        let q_int = q as i32;
        for i in 0..=q_int {
            p += cwilcox(i, mm, nn) / denom;
        }
    } else {
        let q_int = (m * n - q) as i32;
        for i in 0..q_int {
            p += cwilcox(i, mm, nn) / denom;
        }
        lower_tail = !lower_tail; // p = 1 - p;
    }

    r_dt_val(p, lower_tail, log_p)
}

// =====================================================================
// qwilcox
// =====================================================================

pub fn qwilcox_inner(x: f64, m: f64, n: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(m) || isnan(n) {
        return x + m + n;
    }
    if !r_finite(x) || !r_finite(m) || !r_finite(n) {
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

    let m = r_forceint(m);
    let n = r_forceint(n);
    if m <= 0.0 || n <= 0.0 {
        return ml_warn_return_nan();
    }

    // R_Q_P01_boundaries(p, 0, m*n)
    // Check boundary values
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
        return m * n;
    }

    let mut x = x;
    if log_p || !lower_tail {
        x = r_dt_qiv(x, lower_tail, log_p); // lower_tail, non-log "p"
    }

    let mm = m as i32;
    let nn = n as i32;

    let denom = exp(lchoose(m + n, n));
    let mut p = 0.0;
    let mut q: i32 = 0;

    if x <= 0.5 {
        x -= 10.0 * DBL_EPSILON;
        loop {
            p += cwilcox(q, mm, nn) / denom;
            if p >= x {
                break;
            }
            q += 1;
        }
    } else {
        x = 1.0 - x + 10.0 * DBL_EPSILON;
        loop {
            p += cwilcox(q, mm, nn) / denom;
            if p > x {
                q = (m * n - q as f64) as i32;
                break;
            }
            q += 1;
        }
    }

    q as f64
}

// =====================================================================
// rwilcox
// =====================================================================

/// r_unif_index: generate a random non-negative integer < dn
/// using rejection sampling from integers below the next larger power of two.
/// Ported from R's standalone/sunif.c
fn r_unif_index(dn: i32) -> i32 {
    if dn <= 0 {
        return 0;
    }
    let bits = ceil(log2(dn as f64)) as i32;
    loop {
        let mut v: i64 = 0;
        let mut n = 0;
        while n <= bits {
            let v1 = (unif_rand() * 65536.0) as i64;
            v = 65536 * v + v1;
            n += 16;
        }
        // mask out the bits that are not needed
        v &= (1i64 << bits) - 1;
        if (dn as i64) > v {
            return v as i32;
        }
    }
}

pub fn rwilcox_inner(m: f64, n: f64) -> f64 {
    // IEEE_754
    if isnan(m) || isnan(n) {
        return m + n;
    }
    let m = r_forceint(m);
    let n = r_forceint(n);
    if m < 0.0 || n < 0.0 {
        return ml_warn_return_nan();
    }

    if m == 0.0 || n == 0.0 {
        return 0.0;
    }

    let mut r = 0.0;
    let k = (m + n) as i32;
    let mut x: Vec<i32> = (0..k).collect();

    let mut remaining = k;
    for _i in 0..(n as i32) {
        let j = r_unif_index(remaining);
        r += x[j as usize] as f64;
        remaining -= 1;
        x[j as usize] = x[remaining as usize];
    }

    r - n * (n - 1.0) / 2.0
}

// =====================================================================
// FFI shims
// =====================================================================

#[unsafe(no_mangle)]
pub extern "C" fn Rf_dwilcox(x: c_double, m: c_double, n: c_double, give_log: c_int) -> c_double {
    dwilcox_inner(x, m, n, give_log != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn dwilcox(x: c_double, m: c_double, n: c_double, give_log: c_int) -> c_double {
    dwilcox_inner(x, m, n, give_log != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_pwilcox(
    q: c_double,
    m: c_double,
    n: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    pwilcox_inner(q, m, n, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn pwilcox(
    q: c_double,
    m: c_double,
    n: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    pwilcox_inner(q, m, n, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_qwilcox(
    p: c_double,
    m: c_double,
    n: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    qwilcox_inner(p, m, n, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn qwilcox(
    p: c_double,
    m: c_double,
    n: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    qwilcox_inner(p, m, n, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_rwilcox(m: c_double, n: c_double) -> c_double {
    rwilcox_inner(m, n)
}

#[unsafe(no_mangle)]
pub extern "C" fn rwilcox(m: c_double, n: c_double) -> c_double {
    rwilcox_inner(m, n)
}
