// Ported from R's appl/pretty.c
//
// Pretty Intervals: constructs "pretty" values which cover the given interval.

use crate::utils::*;
use libm::*;

const ROUNDING_EPS: f64 = 1e-10;

/// Compute "pretty" axis breakpoints covering the interval [*lo, *up].
///
/// Constructs values that cover the given interval with approximately
/// `*ndiv + 1` intervals.
///
/// # Arguments
/// * `lo` - lower bound of interval (modified on output)
/// * `up` - upper bound of interval (modified on output)
/// * `ndiv` - approximate number of intervals (modified on output)
/// * `min_n` - minimum number of intervals
/// * `shrink_sml` - factor by which scale is shrunk for small intervals
/// * `high_u_fact` - array [h, h5, f_min] controlling unit selection bias
/// * `eps_correction` - whether to apply epsilon correction (0, 1, or 2)
/// * `return_bounds` - if true, lo/up are bounds; if false, lo/up are ns/nu
///
/// # Returns
/// The unit (spacing) used for the pretty values.
#[unsafe(no_mangle)]
pub extern "C" fn R_pretty(
    lo: *mut f64,
    up: *mut f64,
    ndiv: *mut std::os::raw::c_int,
    min_n: std::os::raw::c_int,
    shrink_sml: f64,
    high_u_fact: *const f64,
    eps_correction: std::os::raw::c_int,
    return_bounds: std::os::raw::c_int,
) -> f64 {
    let h = unsafe { *high_u_fact.add(0) };
    let h5 = unsafe { *high_u_fact.add(1) };
    let f_min = unsafe { *high_u_fact.add(2) };

    let lo_ = unsafe { *lo };
    let up_ = unsafe { *up };
    let ndiv_val = unsafe { *ndiv } as i32;
    let dx = up_ - lo_;

    let cell: f64;
    let i_small: bool;

    let dbL_EPSILON: f64 = 2.220446049250313e-16;
    let dbL_MAX: f64 = 1.7976931348623157e+308;
    let dbL_MIN: f64 = 2.2250738585072014e-308;

    if dx == 0.0 && up_ == 0.0 {
        cell = 1.0;
        i_small = true;
    } else {
        cell = fmax2(fabs(lo_), fabs(up_));
        let u = 1.0
            + if h5 >= 1.5 * h + 0.5 {
                1.0 / (1.0 + h)
            } else {
                1.5 / (1.0 + h5)
            };
        let u = u * imax2(1, ndiv_val) as f64 * dbL_EPSILON;
        i_small = dx < cell * u * 3.0;
    }

    let cell = if i_small {
        let mut c = cell;
        if c > 10.0 {
            c = 9.0 + c / 10.0;
        }
        c *= shrink_sml;
        if min_n > 1 {
            c /= min_n as f64;
        }
        c
    } else {
        let mut c = dx;
        if c.is_finite() {
            if ndiv_val > 1 {
                c /= ndiv_val as f64;
            }
        } else {
            // up - lo = +Inf (overflow)
            if ndiv_val < 2 {
                eprintln!(
                    "R_pretty(): infinite range; ndiv={}, should have ndiv >= 2",
                    ndiv_val
                );
            } else {
                c = up_ / (ndiv_val as f64) - lo_ / (ndiv_val as f64);
            }
        }
        c
    };

    let max_f: f64 = 1.25;
    let mut cell = cell;

    let subsmall = f_min * dbL_MIN;
    let subsmall = if subsmall == 0.0 { dbL_MIN } else { subsmall };

    if cell < subsmall {
        if cell > 0.0 {
            eprintln!(
                "R_pretty(): very small range 'cell'={}, increased to {}",
                cell, subsmall
            );
        }
        cell = subsmall;
    } else if cell > dbL_MAX / max_f {
        eprintln!(
            "R_pretty(): very large range 'cell'={}, decreased to {}",
            cell,
            dbL_MAX / max_f
        );
        cell = dbL_MAX / max_f;
    }

    let base = pow(10.0, floor(log10(cell))); // base <= cell < 10*base

    // unit: from {1, 2, 5, 10} * base
    let mut unit = base;
    {
        let u_val = 2.0 * base;
        if u_val - cell < h * (cell - unit) {
            unit = u_val;
            let u_val = 5.0 * base;
            if u_val - cell < h5 * (cell - unit) {
                unit = u_val;
                let u_val = 10.0 * base;
                if u_val - cell < h * (cell - unit) {
                    unit = u_val;
                }
            }
        }
    }

    let mut ns = floor(lo_ / unit + ROUNDING_EPS);
    let mut nu = ceil(up_ / unit - ROUNDING_EPS);

    if eps_correction > 0 && (eps_correction > 1 || !i_small) {
        let d_max = dbL_MAX * (1.0 - ldexp(dbL_EPSILON, -1));
        unsafe {
            if *lo < 0.0 {
                *lo *= 1.0 + dbL_EPSILON;
            } else if *lo > 0.0 {
                *lo *= 1.0 - dbL_EPSILON;
            } else {
                *lo = -fmin2(unit, dbL_MIN);
            }

            if *up < 0.0 {
                *up *= 1.0 - dbL_EPSILON;
            } else if *up > 0.0 {
                if *up < d_max {
                    *up *= 1.0 + dbL_EPSILON;
                }
            } else {
                *up = fmin2(unit, dbL_MIN);
            }
        }
    }

    while ns * unit > unsafe { *lo } + ROUNDING_EPS * unit {
        ns -= 1.0;
    }
    while !(ns * unit).is_finite() {
        ns += 1.0;
    }
    while nu * unit < unsafe { *up } - ROUNDING_EPS * unit {
        nu += 1.0;
    }
    while !(nu * unit).is_finite() {
        nu -= 1.0;
    }

    let k = (0.5 + nu - ns) as i32;

    let new_ndiv = if k < min_n {
        let diff = min_n - k;
        if lo_ == 0.0 && ns == 0.0 && up_ != 0.0 {
            nu += diff as f64;
        } else if up_ == 0.0 && nu == 0.0 && lo_ != 0.0 {
            ns -= diff as f64;
        } else if ns >= 0.0 {
            nu += (diff / 2) as f64;
            ns -= (diff / 2 + diff % 2) as f64;
        } else {
            ns -= (diff / 2) as f64;
            nu += (diff / 2 + diff % 2) as f64;
        }
        min_n
    } else {
        k
    };

    unsafe {
        if return_bounds != 0 {
            if ns * unit < *lo {
                *lo = ns * unit;
            }
            if nu * unit > *up {
                *up = nu * unit;
            }
        } else {
            *lo = ns;
            *up = nu;
        }
        *ndiv = new_ndiv;
    }

    unit
}
