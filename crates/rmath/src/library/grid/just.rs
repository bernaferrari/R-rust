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
pub fn justifyX(x: f64, width: f64, hjust: f64) -> f64 {
    x - width * hjust
}

pub fn justifyY(y: f64, height: f64, vjust: f64) -> f64 {
    y - height * vjust
}

/* Convert enum justification into 0..1 justification */

/* Return the amount of justification required */
pub fn justification(width: f64, height: f64, hjust: f64, vjust: f64) -> (f64, f64) {
    (-width * hjust, -height * vjust)
}
