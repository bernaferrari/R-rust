/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2011   The R Core Team.
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
 *  Ported from r-source/src/library/parallel/src/rngstream.c
 */

use std::os::raw::c_int;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::protect::*;

type Uint64 = u64;

const A1P76: [[Uint64; 3]; 3] = [
    [82758667, 1871391091, 4127413238],
    [3672831523, 69195019, 1871391091],
    [3672091415, 3528743235, 69195019],
];

const A2P76: [[Uint64; 3]; 3] = [
    [1511326704, 3759209742, 1610795712],
    [4292754251, 1511326704, 3889917532],
    [3859662829, 4292754251, 3708466080],
];

const A1P127: [[Uint64; 3]; 3] = [
    [2427906178, 3580155704, 949770784],
    [226153695, 1230515664, 3580155704],
    [1988835001, 986791581, 1230515664],
];

const A2P127: [[Uint64; 3]; 3] = [
    [1464411153, 277697599, 1610723613],
    [32183930, 1464411153, 1022607788],
    [2824425944, 32183930, 2093834863],
];

fn advance_stream(x: SEXP, a1: &[[Uint64; 3]; 3], a2: &[[Uint64; 3]; 3]) -> SEXP {
    let x_vals: Vec<c_int> = (0..7).map(|i| INTEGER_ELT(x, i)).collect();
    let seed = &x_vals[1..];
    let mut nseed: [Uint64; 6] = [0; 6];

    for i in 0..3 {
        let mut tmp: Uint64 = 0;
        for j in 0..3 {
            tmp += a1[i][j] * seed[j] as Uint64;
            tmp %= 4294967087;
        }
        nseed[i] = tmp;
    }

    for i in 0..3 {
        let mut tmp: Uint64 = 0;
        for j in 0..3 {
            tmp += a2[i][j] * seed[j + 3] as Uint64;
            tmp %= 4294944443;
        }
        nseed[i + 3] = tmp;
    }

    let ans = Rf_allocVector(SEXPTYPE::INTSXP, 7);
    SET_INTEGER_ELT(ans, 0, x_vals[0]);
    for (i, value) in nseed.iter().enumerate() {
        SET_INTEGER_ELT(ans, (i + 1) as c_int, *value as c_int);
    }
    ans
}


