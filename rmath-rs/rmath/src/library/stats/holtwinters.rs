
//! Holt-Winters filtering algorithm.
//! Port of r-source/src/library/stats/src/HoltWinters.c

use core::ffi::{c_double, c_int, c_void};

/// Holt-Winters filtering.
///
/// Port of `HoltWinters` from R's `src/library/stats/src/HoltWinters.c`.
///
/// # Safety
/// All pointer arguments must be valid and point to appropriately sized arrays.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn HoltWinters(
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
                + (1.0 - *alpha) * (*level.add((i0 - 1) as usize) + *trend.add((i0 - 1) as usize));
        } else {
            *level.add(i0 as usize) = *alpha * (*x.add(i as usize) / stmp)
                + (1.0 - *alpha) * (*level.add((i0 - 1) as usize) + *trend.add((i0 - 1) as usize));
        }

        if dotrend_val == 1 {
            *trend.add(i0 as usize) = *beta
                * (*level.add(i0 as usize) - *level.add((i0 - 1) as usize))
                + (1.0 - *beta) * *trend.add((i0 - 1) as usize);
        }

        if doseasonal_val == 1 {
            if seasonal_val == 1 {
                *season.add(s0 as usize) =
                    *gamma * (*x.add(i as usize) - *level.add(i0 as usize)) + (1.0 - *gamma) * stmp;
            } else {
                *season.add(s0 as usize) =
                    *gamma * (*x.add(i as usize) / *level.add(i0 as usize)) + (1.0 - *gamma) * stmp;
            }
        }

        i += 1;
    }
}
