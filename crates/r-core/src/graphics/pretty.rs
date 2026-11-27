//! GNU's pretty tick computation — ported operation-for-operation from
//! appl/pretty.rs, itself a faithful port of R's pretty.c. Exact
//! floating-point operation order is semantic: never simplify.

/// Machine constants exactly as the original translation unit.
const DBL_EPSILON: f64 = 2.220446049250313e-16;
const DBL_MAX: f64 = 1.7976931348623157e+308;
const DBL_MIN: f64 = 2.2250738585072014e-308;
/// R's rounding tolerance exactly as pretty.c's roundingEps.
const ROUNDING_EPS: f64 = 1e-10;

fn ldexp(x: f64, exp: i32) -> f64 {
    x * 2f64.powi(exp)
}

/// The result of one pretty computation: adjusted bounds, interval
/// count and cell unit exactly as R_pretty's in/out contract.
pub struct PrettyResult {
    pub lo: f64,
    pub up: f64,
    pub ndiv: i32,
    pub unit: f64,
}

/// GNU's R_pretty exactly: compute "nice" tick coverage of [lo, up]
/// with about ndiv intervals, never fewer than min_n. h/h5 are the
/// unit-preference biases, f_min the small-range clamp factor, and
/// eps_correction nudges bounds outward when set. Bounds tighten to
/// exact multiples exactly as return_bounds=1.
#[allow(clippy::too_many_arguments)]
pub fn r_pretty(
    mut lo: f64,
    mut up: f64,
    ndiv: i32,
    min_n: i32,
    shrink_sml: f64,
    h: f64,
    h5: f64,
    f_min: f64,
    eps_correction: i32,
) -> PrettyResult {
    let lo_ = lo;
    let up_ = up;
    let ndiv_val = ndiv;
    let dx = up_ - lo_;

    let cell: f64;
    let i_small: bool;

    if dx == 0.0 && up_ == 0.0 {
        cell = 1.0;
        i_small = true;
    } else {
        cell = lo_.abs().max(up_.abs());
        let u = 1.0
            + if h5 >= 1.5 * h + 0.5 {
                1.0 / (1.0 + h)
            } else {
                1.5 / (1.0 + h5)
            };
        let u = u * ndiv_val.max(1) as f64 * DBL_EPSILON;
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
        } else if ndiv_val >= 2 {
            c = up_ / (ndiv_val as f64) - lo_ / (ndiv_val as f64);
        }
        c
    };

    let max_f: f64 = 1.25;
    let mut cell = cell;

    let subsmall = f_min * DBL_MIN;
    let subsmall = if subsmall == 0.0 { DBL_MIN } else { subsmall };

    // GNU warns through the error channel for extreme cells; the pure
    // world clamps silently exactly as far as results are concerned.
    if cell < subsmall {
        cell = subsmall;
    } else if cell > DBL_MAX / max_f {
        cell = DBL_MAX / max_f;
    }

    let base = 10f64.powf(cell.log10().floor()); // base <= cell < 10*base

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

    let lo_use = if lo_.is_finite() {
        lo_
    } else if lo_.is_sign_negative() {
        -cell
    } else {
        cell
    };
    let up_use = if up_.is_finite() {
        up_
    } else if up_.is_sign_negative() {
        -cell
    } else {
        cell
    };
    let mut ns = (lo_use / unit + ROUNDING_EPS).floor();
    let mut nu = (up_use / unit - ROUNDING_EPS).ceil();

    if !ns.is_finite() {
        ns = -(DBL_MAX / unit).floor();
    }
    if !nu.is_finite() {
        nu = (DBL_MAX / unit).floor();
    }

    if eps_correction > 0 && (eps_correction > 1 || !i_small) {
        let d_max = DBL_MAX * (1.0 - ldexp(DBL_EPSILON, -1));
        if lo < 0.0 {
            lo *= 1.0 + DBL_EPSILON;
        } else if lo > 0.0 {
            lo *= 1.0 - DBL_EPSILON;
        } else {
            lo = -unit.min(DBL_MIN);
        }

        if up < 0.0 {
            up *= 1.0 - DBL_EPSILON;
        } else if up > 0.0 {
            if up < d_max {
                up *= 1.0 + DBL_EPSILON;
            }
        } else {
            up = unit.min(DBL_MIN);
        }
    }

    let mut guard = 0;
    while ns * unit > lo + ROUNDING_EPS * unit && guard < 10000 {
        ns -= 1.0;
        guard += 1;
    }
    guard = 0;
    while !(ns * unit).is_finite() && ns.is_finite() && guard < 10000 {
        ns += 1.0;
        guard += 1;
    }
    guard = 0;
    while nu * unit < up - ROUNDING_EPS * unit && guard < 10000 {
        nu += 1.0;
        guard += 1;
    }
    guard = 0;
    while !(nu * unit).is_finite() && nu.is_finite() && guard < 10000 {
        nu -= 1.0;
        guard += 1;
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

    if ns * unit < lo {
        lo = ns * unit;
    }
    if nu * unit > up {
        up = nu * unit;
    }

    PrettyResult {
        lo,
        up,
        ndiv: new_ndiv,
        unit,
    }
}
