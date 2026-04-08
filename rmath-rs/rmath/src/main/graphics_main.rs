#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1997--2021  The R Core Team
 *  Copyright (C) 2002--2011  The R Foundation
 *  Copyright (C) 1995, 1996  Robert Gentleman and Ross Ihaka
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 *
 *  Ported from r-source/src/main/graphics.c
 *
 *  GAxisPars, GLPretty, GPretty -- axis parameter computation functions.
 *  Used in GScale() (../library/graphics/src/graphics.c) and also in
 *  ../library/grDevices/src/axis_scales.c.
 */

use std::os::raw::{c_char, c_double, c_int};

use crate::main::engine::GEPretty;
use crate::nmath::utils::fmax2;

/// Rboolean type (0 = FALSE, 1 = TRUE).
type Rboolean = c_int;

/// Rexp10 -- compute 10^x. Equivalent to C's `#define Rexp10(x) pow(10.0, x)`.
#[inline]
fn Rexp10(x: c_double) -> c_double {
    10.0_f64.powf(x)
}

/// GAxisPars -- compute axis parameters from user range.
///
/// (usr = (min,max), n_inp, log) -> (axp = (min, max), n_out)
///
/// Used in GScale() (../library/graphics/src/graphics.c) and
/// in ../library/grDevices/src/axis_scales.c.
pub unsafe fn GAxisPars(
    min: *mut c_double,
    max: *mut c_double,
    n: *mut c_int,
    log: Rboolean,
    axis: c_int,
) {
    unsafe {
        const EPS_FAC_2: c_double = 16.0;

        let swap = *min > *max;

        /* MAYBE_SWAP macro */
        if swap {
            let t = *min;
            *min = *max;
            *max = t;
        }

        /* save only for the extreme case */
        let min_o = *min;
        let max_o = *max;

        if log != 0 {
            /* Avoid infinities */
            if *max > 308.0 {
                *max = 308.0;
                if *min > *max {
                    *min = *max;
                }
            }
            if *min < -307.0 {
                *min = -307.0;
                if *max < *min {
                    *max = *min;
                }
            }
            *min = Rexp10(*min);
            *max = Rexp10(*max);
            GLPretty(min, max, n);
        } else {
            GEPretty(min, max, n);
        }

        let t_ = fmax2((*max).abs(), (*min).abs());
        let tf = if t_ > 1.0 {
            (t_ * f64::EPSILON) * EPS_FAC_2
        } else {
            (t_ * EPS_FAC_2) * f64::EPSILON
        };
        let tf = if tf == 0.0 { f64::MIN_POSITIVE } else { tf };

        if ((*max - *min).abs()) <= tf {
            /* Too much accuracy just shows machine differences */
            if axis != 0 {
                crate::main::errors::Rf_warning(
                    b"axis(%d, *): range of values is small wrt |M| = ... --> not pretty()\0"
                        .as_ptr() as *const c_char,
                );
            }

            /* No pretty()ing anymore */
            *min = min_o;
            *max = max_o;
            let eps = 0.005 * (*max - *min);
            *min += eps;
            *max -= eps;
            if log != 0 {
                *min = Rexp10(*min);
                *max = Rexp10(*max);
            }
            *n = 1;
        }

        /* MAYBE_SWAP back */
        if swap {
            let t = *min;
            *min = *max;
            *max = t;
        }
    }
}

const LPR_SMALL: c_int = 2;
const LPR_MEDIUM: c_int = 3;

/// GLPretty -- generate pretty tick values for LOGARITHMIC scale.
/// ul < uh. This only does a very simple setup.
/// The real work happens when the axis is drawn.
unsafe fn GLPretty(ul: *mut c_double, uh: *mut c_double, n: *mut c_int) {
    unsafe {
        let dl = *ul;
        let dh = *uh;
        let mut p1 = dl.log10().ceil() as c_int;
        let mut p2 = dh.log10().floor() as c_int;

        if p2 <= p1 && dh / dl > 10.0 {
            p1 = (dl.log10() - 0.5).ceil() as c_int;
            p2 = (dh.log10() + 0.5).floor() as c_int;
        }

        if p2 <= p1 {
            /* Very small range: use tickmarks from a LINEAR scale */
            GPretty(ul, uh, n);
            *n = -*n;
        } else {
            /* Extra tickmarks -> CreateAtVector() in ./plot.c */
            /* round to nice "1e<N>" */
            *ul = Rexp10(p1 as c_double);
            *uh = Rexp10(p2 as c_double);
            if p2 - p1 <= LPR_SMALL {
                *n = 3; /* Small range: use 1,2,5,10 times 10^k tickmarks */
            } else if p2 - p1 <= LPR_MEDIUM {
                *n = 2; /* Medium range: use 1,5 times 10^k tickmarks */
            } else {
                *n = 1; /* Large range: use 10^k tickmarks */
            }
        }
    }
}

/// GPretty -- compute "pretty" axis label positions.
/// Delegates to GEPretty (in engine.c, calling R_pretty()).
#[unsafe(no_mangle)]
pub unsafe fn GPretty(lo: *mut c_double, up: *mut c_double, ndiv: *mut c_int) {
    unsafe {
        GEPretty(lo, up, ndiv);
    }
}
