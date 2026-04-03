#![allow(unsafe_op_in_unsafe_fn)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001-3 Paul Murrell
 *                2003 The R Core Team
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
 */

/* Modify a location for the correct justification */

use std::os::raw::c_int;

const L_BOTTOM: c_int = 0;
const L_LEFT: c_int = 0;
const L_CENTRE: c_int = 1;
const L_CENTER: c_int = 1;
const L_TOP: c_int = 2;
const L_RIGHT: c_int = 2;

/* These transformations assume that x and width are in the same units */
#[unsafe(no_mangle)]
pub unsafe extern "C" fn justifyX(x: f64, width: f64, hjust: f64) -> f64 {
    x - width * hjust
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn justifyY(y: f64, height: f64, vjust: f64) -> f64 {
    y - height * vjust
}

/* Convert enum justification into 0..1 justification */
#[unsafe(no_mangle)]
pub unsafe extern "C" fn convertJust(just: c_int) -> f64 {
    let mut result: f64 = 0.0;
    match just {
        _ if just == L_BOTTOM || just == L_LEFT => {
            result = 0.0;
        }
        _ if just == L_CENTRE || just == L_CENTER => {
            result = 0.5;
        }
        _ if just == L_TOP || just == L_RIGHT => {
            result = 1.0;
        }
        _ => {}
    }
    result
}

/* Return the amount of justification required */
#[unsafe(no_mangle)]
pub unsafe extern "C" fn justification(
    width: f64,
    height: f64,
    hjust: f64,
    vjust: f64,
    hadj: *mut f64,
    vadj: *mut f64,
) {
    *hadj = -width * hjust;
    *vadj = -height * vjust;
}
