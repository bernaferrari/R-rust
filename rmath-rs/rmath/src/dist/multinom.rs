// Multinomial distribution: rmultinom
// Ported from rmultinom.c
//   Reference: Uses sequential binomial sampling.

use crate::constants::*;
use crate::dist::binomial::rbinom_inner;
use crate::error::*;
use libm::fabs;
use std::os::raw::{c_double, c_int};

// ---- rmultinom ----

/// Generate a random vector from the multinomial distribution.
/// `rn` is filled with `K` values where rn[j] ~ Bin(n, prob[j]),
/// sum_j rn[j] == n, sum_j prob[j] == 1.
pub fn rmultinom_inner(n: i32, prob: &[f64], rn: &mut [f64]) {
    let k = prob.len();

    if k < 1 {
        ml_warning(ME_DOMAIN, "rmultinom");
        return;
    }
    if n < 0 {
        ml_warning(ME_DOMAIN, "rmultinom");
        #[allow(clippy::len_zero)]
        if rn.len() > 0 {
            rn[0] = -1.0;
        }
        return;
    }

    let mut p_tot: f64 = 0.0;

    // Validate probabilities and accumulate total
    for i in 0..k {
        let pp = prob[i];
        if !r_finite(pp) || pp < 0.0 || pp > 1.0 {
            ml_warning(ME_DOMAIN, "rmultinom");
            if i < rn.len() {
                rn[i] = -1.0;
            }
            return;
        }
        p_tot += pp;
        if i < rn.len() {
            rn[i] = 0.0;
        }
    }

    // Check probability sum (with tolerance)
    if fabs(p_tot - 1.0) > 1e-7 {
        // In R this calls MATHLIB_ERROR, here we just warn
        ml_warning(ME_DOMAIN, "rmultinom");
    }

    if n == 0 {
        return;
    }
    if k == 1 && p_tot == 0.0 {
        return;
    } /* trivial border case: do as rbinom */

    // Generate the first K-1 obs. via binomials
    let mut n_remaining = n;
    let mut p_remaining = p_tot;

    for i in 0..k - 1 {
        if prob[i] != 0.0 {
            let pp = prob[i] / p_remaining;
            let val = if pp < 1.0 {
                rbinom_inner(n_remaining as f64, pp)
            } else {
                // >= 1; > 1 happens because of rounding
                n_remaining as f64
            };
            let count = val as i32;
            if i < rn.len() {
                rn[i] = count as f64;
            }
            n_remaining -= count;
        } else {
            if i < rn.len() {
                rn[i] = 0.0;
            }
        }
        if n_remaining <= 0 {
            return;
        } /* we have all */
        p_remaining -= prob[i]; /* i.e. = sum(prob[(i+1):K]) */
    }

    // Last category gets the remainder
    if (k - 1) < rn.len() {
        rn[k - 1] = n_remaining as f64;
    }
}

// ---- FFI shim ----

pub extern "C" fn rmultinom(n: c_int, prob: *const c_double, k: c_int, rn: *mut c_double) {
    if prob.is_null() || rn.is_null() || k < 0 {
        return;
    }
    let k_usize = k as usize;
    let n_i32 = n;

    unsafe {
        let prob_slice = std::slice::from_raw_parts(prob, k_usize);
        let rn_slice = std::slice::from_raw_parts_mut(rn, k_usize);
        rmultinom_inner(n_i32, prob_slice, rn_slice);
    }
}

pub extern "C" fn Rf_rmultinom(n: c_int, prob: *const c_double, k: c_int, rn: *mut c_double) {
    rmultinom(n, prob, k, rn);
}
