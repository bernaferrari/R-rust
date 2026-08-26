// Ported from R's nmath/choose.c
//
// Binomial coefficients.
// choose(n, k)   and  lchoose(n,k) := log(abs(choose(n,k))
//
// These work for the *generalized* binomial theorem,
// i.e., are also defined for non-integer n (integer k).
//
// We use the simple explicit product formula for k <= k_small_max
// and also have added statements to make sure that the symmetry
//   (n \\ k ) == (n \\ n-k)  is preserved for non-negative integer n.

use crate::constants::*;
use crate::special::gamma::lgammafn;
use crate::special::lbeta::lbeta;
use crate::utils::*;
use libm::{exp, fabs, floor, log};

const K_SMALL_MAX: i32 = 30;

fn odd(k: f64) -> bool {
    k != 2.0 * floor(k / 2.0)
}

fn r_is_int(x: f64) -> bool {
    !r_nonint(x)
}

fn lfastchoose(n: f64, k: f64) -> f64 {
    -log(n + 1.0) - lbeta(n - k + 1.0, k + 1.0)
}

/// mathematically the same as lfastchoose:
/// less stable typically, but useful if n-k+1 < 0.
/// Returns (log_value, sign).
fn lfastchoose2(n: f64, k: f64) -> (f64, i32) {
    let r = lgammafn(n + 1.0) - lgammafn(k + 1.0) - lgammafn(n - k + 1.0);
    // Determine sign of gamma(n-k+1) via reflection formula.
    let s = gamma_sign(n - k + 1.0);
    (r, s)
}

/// Determine the sign of gamma(x).
fn gamma_sign(x: f64) -> i32 {
    if x > 0.0 {
        return 1;
    }
    // For x < 0: gamma(x) = pi / (sin(pi*x) * gamma(1-x))
    let s = libm::sin(std::f64::consts::PI * x);
    if s >= 0.0 { 1 } else { -1 }
}

mod imp {
    use super::*;

    pub fn lchoose(n: f64, k: f64) -> f64 {
        let _k0 = k;
        let k = r_forceint(k);

        // NaNs propagated correctly
        if isnan(n) || isnan(k) {
            return n + k;
        }

        if k < 2.0 {
            if k < 0.0 {
                return ML_NEGINF;
            }
            if k == 0.0 {
                return 0.0;
            }
            // else: k == 1
            return log(fabs(n));
        }
        // else: k >= 2

        if n < 0.0 {
            return lchoose(-n + k - 1.0, k);
        } else if r_is_int(n) {
            let n = r_forceint(n);
            if n < k {
                return ML_NEGINF;
            }
            // k <= n
            if n - k < 2.0 {
                return lchoose(n, n - k); // <- Symmetry
            }
            // else: n >= k+2
            return lfastchoose(n, k);
        }
        // else non-integer n >= 0
        if n < k - 1.0 {
            let (r, _s) = lfastchoose2(n, k);
            return r;
        }
        lfastchoose(n, k)
    }

    pub fn choose(n: f64, k: f64) -> f64 {
        let _k0 = k;
        let k = r_forceint(k);

        // NaNs propagated correctly
        if isnan(n) || isnan(k) {
            return n + k;
        }

        if k < K_SMALL_MAX as f64 {
            // Symmetry: ensure k still integer
            let mut k_val = k;
            if n - k < k && n >= 0.0 && r_is_int(n) {
                k_val = r_forceint(n - k);
            }
            if k_val < 0.0 {
                return 0.0;
            }
            if k_val == 0.0 {
                return 1.0;
            }
            // else: k_val >= 1
            let mut r = n;
            let ki = k_val as i32;
            let mut j: i32 = 2;
            while j <= ki {
                r *= (n - j as f64 + 1.0) / j as f64;
                j += 1;
            }
            return if r_is_int(n) { r_forceint(r) } else { r };
        }
        // else: k >= k_small_max

        if n < 0.0 {
            let r = choose(-n + k - 1.0, k);
            if odd(k) { -r } else { r }
        } else if r_is_int(n) {
            let n = r_forceint(n);
            if n < k {
                return 0.0;
            }
            if n - k < K_SMALL_MAX as f64 {
                return choose(n, n - k); // <- Symmetry
            }
            r_forceint(exp(lfastchoose(n, k)))
        }
        // else non-integer n >= 0
        else if n < k - 1.0 {
            let (r, s) = lfastchoose2(n, k);
            s as f64 * exp(r)
        } else {
            exp(lfastchoose(n, k))
        }
    }
}

// =====================================================================
// Public API (Rust) -- delegates to inner module
// =====================================================================

/// Log of the absolute value of the binomial coefficient.
/// lchoose(n, k) := log(|choose(n, k)|)
pub fn lchoose(n: f64, k: f64) -> f64 {
    imp::lchoose(n, k)
}

/// Binomial coefficient C(n, k).
pub fn choose(n: f64, k: f64) -> f64 {
    imp::choose(n, k)
}

// =====================================================================
// C FFI shims
// =====================================================================

pub fn Rf_choose(n: f64, k: f64) -> f64 {
    imp::choose(n, k)
}

pub fn choose_c(n: f64, k: f64) -> f64 {
    imp::choose(n, k)
}

pub fn Rf_lchoose(n: f64, k: f64) -> f64 {
    imp::lchoose(n, k)
}

pub fn lchoose_c(n: f64, k: f64) -> f64 {
    imp::lchoose(n, k)
}

/// Rf_lfastchoose: fast version of lchoose(n, k) for integer k.
/// Returns log|choose(n,k)|. The sign argument is for compatibility only.
pub fn Rf_lfastchoose(n: f64, k: f64, _sgn: *mut std::os::raw::c_int) -> f64 {
    imp::lchoose(n, k)
}
