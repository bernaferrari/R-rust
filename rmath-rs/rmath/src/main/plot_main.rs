#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1997--2021  The R Core Team
 *  Copyright (C) 2002--2009  The R Foundation
 *  Copyright (C) 1995, 1996  Robert Gentleman and Ross Ihaka
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 3 of the License, or
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
 *  This is an extensive reworking by Paul Murrell of an original
 *  quick hack by Ross Ihaka designed to give a superset of the
 *  functionality in the AT&T Bell Laboratories GRZ library.
 *
 *  Ported from r-source/src/main/plot.c
 */

use std::os::raw::{c_char, c_double, c_int};

use crate::main::errors::Rf_error;
use crate::nmath::utils::imax2;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::R_NilValue;

/// Rboolean type (0 = FALSE, 1 = TRUE).
type Rboolean = c_int;

/// Rexp10 -- compute 10^x. Equivalent to C's `#define Rexp10(x) pow(10.0, x)`.
#[inline]
fn Rexp10(x: c_double) -> c_double {
    10.0_f64.powf(x)
}

/// R_FINITE -- check if a double is finite (not NA, NaN, Inf, -Inf).
#[inline]
fn R_FINITE(x: c_double) -> bool {
    x.is_finite()
}

/// Helper: write to REAL(x)[i]
#[inline]
unsafe fn SET_REAL(x: SEXP, i: usize, val: c_double) {
    unsafe {
        std::ptr::write(REAL(x).add(i), val);
    }
}

/// Helper: read from REAL(x)[i]
#[inline]
unsafe fn GET_REAL(x: SEXP, i: usize) -> c_double {
    unsafe { std::ptr::read(REAL(x).add(i)) }
}

/// CreateAtVector -- create an 'at = ...' vector for axis(.),
/// i.e., the vector of tick mark locations, when none has been specified.
///
/// This is used in graphics and grid.
pub unsafe fn CreateAtVector(
    axp: *mut c_double,
    usr: *const c_double,
    nint: c_int,
    logflag: Rboolean,
) -> SEXP {
    unsafe {
        // "arbitrary" threshold: |delta_tick| / SMALL is "barely visible" in plot
        const SMALL_F: c_double = 100.0;

        let mut at: SEXP = R_NilValue();
        let mut dn: c_double;
        let mut rng: c_double;
        let small: c_double;
        let mut n: c_int;
        let mut i: c_int;
        let mut a_i: c_double;

        if logflag == 0 || *axp.add(2) < 0.0 {
            /* --- linear axis --- Only use axp[] arg. */
            n = (*axp.add(2)).abs() as c_int; /* >= 0, truncates toward zero */
            dn = imax2(1, n) as c_double;
            rng = *axp.add(1) - *axp.add(0);
            at = Rf_allocVector(SEXPTYPE::REALSXP.0 as c_int, n + 1);

            if !R_FINITE(rng) {
                /* need to carefully work around overflow */
                let at_ = *axp.add(0) / dn; /* "/dn" avoids overflow */
                rng = *axp.add(1) / dn - at_;
                small = rng.abs() / SMALL_F;
                let n2 = n / 2; /* integer division */
                i = 0;
                while i <= n2 {
                    a_i = *axp.add(0) + i as c_double * rng;
                    SET_REAL(at, i as usize, if a_i.abs() < small { 0.0 } else { a_i });
                    i += 1;
                }
                let mut i2 = 0;
                while i2 < n - n2 {
                    i = n - i2;
                    a_i = *axp.add(1) - i2 as c_double * rng;
                    SET_REAL(at, i as usize, if a_i.abs() < small { 0.0 } else { a_i });
                    i2 += 1;
                }
            } else {
                /* rng is finite (normal case) */
                small = rng.abs() / SMALL_F / dn;
                i = 0;
                while i <= n {
                    a_i = *axp.add(0) + (i as c_double / dn) * rng;
                    SET_REAL(at, i as usize, if a_i.abs() < small { 0.0 } else { a_i });
                    i += 1;
                }
            }
        } else {
            /* ------ log axis ----- */
            let mut reversed = false;
            let mut umin = *usr.add(0);
            let mut umax = *usr.add(1);
            n = (*axp.add(2) + 0.5) as c_int;

            if umin > umax {
                reversed = *axp.add(0) > *axp.add(1);
                if reversed {
                    /* have *reversed* log axis -- reverse axis direction here,
                     * and reverse back at end */
                    umin = *usr.add(1);
                    umax = *usr.add(0);
                    dn = *axp.add(0);
                    *axp.add(0) = *axp.add(1);
                    *axp.add(1) = dn;
                } else {
                    Rf_error(
                        b"CreateAtVector \"log\"(from axis()): usr[0] > usr[1] !\0".as_ptr()
                            as *const c_char,
                    );
                }
            }

            /* allow a fuzz (iff we don't under-/over-flow) */
            dn = 1.0 - 1e-12;
            if (umin * dn).abs() > 0.0 {
                umin *= dn;
            }
            dn = 1.0 + 1e-12;
            if (umax * dn).abs() <= f64::MAX {
                umax *= dn;
            }

            dn = *axp.add(0);
            if dn < f64::MIN_POSITIVE {
                if dn <= 0.0 {
                    Rf_error(
                        b"CreateAtVector [log-axis()]: axp[0] < 0!\0".as_ptr() as *const c_char
                    );
                } else {
                    crate::main::errors::Rf_warning(
                        b"CreateAtVector [log-axis()]: small axp[0]\0".as_ptr() as *const c_char,
                    );
                }
            }

            match n {
                1 => {
                    /* large range: 1 * 10^k */
                    let i_val =
                        ((*axp.add(1)).log10().floor() - (*axp.add(0)).log10().ceil()) as c_int;
                    let mut ne = i_val / nint;
                    if ne < 1 {
                        ne = 1;
                    } else {
                        let l10_max = umax.log10();
                        let d0 = l10_max - dn.log10();
                        while ne > 1 && (nint * ne) as c_double > d0 {
                            ne -= 1;
                        }
                    }
                    let k = 1 + ne / 308; /* >= 1, typically == 1 */
                    let k = if k < 1 { 1 } else { k };
                    let ne = if k > 1 { k * (ne / k) } else { ne };

                    let l10_max = umax.log10();
                    let d0_val = l10_max - dn.log10();
                    let d1 = d0_val - (nint * ne) as c_double;

                    let mut d0 = dn;
                    const LARGE_D1: c_double = 5.0;
                    if d1 > LARGE_D1 {
                        d0 = dn * Rexp10((d1 / 2.0).floor());
                    }
                    rng = Rexp10(ne as c_double / k as c_double);
                    n = 0;
                    dn = d0;
                    while dn < umax {
                        for _ in 0..k {
                            dn *= rng;
                        }
                        n += 1;
                    }
                    if n == 0 {
                        Rf_error(
                            b"log - axis(), 'at' creation, _LARGE_ range: invalid {xy}axp or par\0"
                                .as_ptr() as *const c_char,
                        );
                    }
                    at = Rf_allocVector(SEXPTYPE::REALSXP.0 as c_int, n);
                    dn = d0;
                    i = 0;
                    while i < n {
                        SET_REAL(at, i as usize, dn);
                        for _ in 0..k {
                            dn *= rng;
                        }
                        i += 1;
                    }
                }
                2 => {
                    /* medium range: 1, 5 * 10^k */
                    n = 0;
                    if 0.5 * dn >= umin {
                        n += 1;
                    }
                    loop {
                        if dn > umax {
                            break;
                        }
                        n += 1;
                        if 5.0 * dn > umax {
                            break;
                        }
                        n += 1;
                        dn *= 10.0;
                    }
                    if n == 0 {
                        Rf_error(
                            b"log - axis(), 'at' creation, _MEDIUM_ range: invalid {xy}axp or par\0"
                                .as_ptr() as *const c_char,
                        );
                    }
                    at = Rf_allocVector(SEXPTYPE::REALSXP.0 as c_int, n);
                    dn = *axp.add(0);
                    n = 0;
                    if 0.5 * dn >= umin {
                        SET_REAL(at, n as usize, 0.5 * dn);
                        n += 1;
                    }
                    loop {
                        if dn > umax {
                            break;
                        }
                        SET_REAL(at, n as usize, dn);
                        n += 1;
                        if 5.0 * dn > umax {
                            break;
                        }
                        SET_REAL(at, n as usize, 5.0 * dn);
                        n += 1;
                        dn *= 10.0;
                    }
                }
                3 => {
                    /* small range: 1, 2, 5, 10 * 10^k */
                    n = 0;
                    if 0.2 * dn >= umin {
                        n += 1;
                    }
                    if 0.5 * dn >= umin {
                        n += 1;
                    }
                    loop {
                        if dn > umax {
                            break;
                        }
                        n += 1;
                        if 2.0 * dn > umax {
                            break;
                        }
                        n += 1;
                        if 5.0 * dn > umax {
                            break;
                        }
                        n += 1;
                        dn *= 10.0;
                    }
                    if n == 0 {
                        Rf_error(
                            b"log - axis(), 'at' creation, _SMALL_ range: invalid {xy}axp or par\0"
                                .as_ptr() as *const c_char,
                        );
                    }
                    at = Rf_allocVector(SEXPTYPE::REALSXP.0 as c_int, n);
                    dn = *axp.add(0);
                    n = 0;
                    if 0.2 * dn >= umin {
                        SET_REAL(at, n as usize, 0.2 * dn);
                        n += 1;
                    }
                    if 0.5 * dn >= umin {
                        SET_REAL(at, n as usize, 0.5 * dn);
                        n += 1;
                    }
                    loop {
                        if dn > umax {
                            break;
                        }
                        SET_REAL(at, n as usize, dn);
                        n += 1;
                        if 2.0 * dn > umax {
                            break;
                        }
                        SET_REAL(at, n as usize, 2.0 * dn);
                        n += 1;
                        if 5.0 * dn > umax {
                            break;
                        }
                        SET_REAL(at, n as usize, 5.0 * dn);
                        n += 1;
                        dn *= 10.0;
                    }
                }
                _ => {
                    Rf_error(
                        b"log - axis(), 'at' creation: INVALID {xy}axp[3]\0".as_ptr()
                            as *const c_char,
                    );
                }
            }

            if reversed {
                /* reverse back again */
                i = 0;
                while i < n / 2 {
                    dn = GET_REAL(at, i as usize);
                    SET_REAL(at, i as usize, GET_REAL(at, (n - i - 1) as usize));
                    SET_REAL(at, (n - i - 1) as usize, dn);
                    i += 1;
                }
            }
        } /* linear / log */
        at
    }
}
