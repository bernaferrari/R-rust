#![allow(unsafe_op_in_unsafe_fn)] // legacy C-port unsafe boundary; see docs/unsafe-op-allowlist.tsv.
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1997--2025  The R Core Team
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
 *  Ported from r-source/src/library/graphics/src/stem.c
 */

use std::os::raw::{c_double, c_int};

use crate::nmath::special::mlutils::R_pow_di;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

unsafe fn asInteger(x: SEXP) -> c_int {
    crate::main::coerce::asInteger(x)
}

unsafe fn asReal(x: SEXP) -> c_double {
    crate::main::coerce::asReal(x)
}

unsafe fn asLogical(x: SEXP) -> c_int {
    crate::main::coerce::asLogical(x)
}

#[unsafe(no_mangle)]
unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::main::coerce::coerceVector(x, type_)
}

unsafe fn R_rsort(x: *mut c_double, n: c_int) {
    crate::main::sort::R_rsort(x, n)
}

#[unsafe(no_mangle)]
unsafe fn Rprintf(msg: &str) -> c_int {
    eprint!("{}", msg);
    0
}

#[inline]
fn imin2(x: c_int, y: c_int) -> c_int {
    if x < y { x } else { y }
}

#[inline]
fn imax2(x: c_int, y: c_int) -> c_int {
    if x < y { y } else { x }
}

unsafe fn stem_print(close: c_int, dist: c_int, ndigits: c_int) {
    if close / 10 == 0 && dist < 0 {
        Rprintf(&format!("  {:1$} | ", "-0", (ndigits + 1) as usize));
    } else {
        Rprintf(&format!("  {:1$} | ", close / 10, (ndigits + 1) as usize));
    }
}

unsafe fn rnd(u: c_double, c: c_double) -> c_double {
    if u < 0.0 { u * c - 0.5 } else { u * c + 0.5 }
}

unsafe fn stem_leaf(x: *mut c_double, n: c_int, scale: c_double, width: c_int, atom: c_double) {
    let mut r: c_double;
    let mut c: c_double;
    let mu: c_double;
    let mut lo: c_double;
    let mut hi: c_double;
    let mm: c_int;
    let k: c_int;
    let mut i: c_int;
    let mut j: c_int;
    let mut xi: c_int;
    let ldigits: c_int;
    let hdigits: c_int;
    let ndigits: c_int;
    let pdigits: c_int;

    R_rsort(x, n);

    Rprintf("\n");
    let mut mu = 10.0_f64;
    if *x.add((n - 1) as usize) > *x.add(0) {
        r = atom + (*x.add((n - 1) as usize) - *x.add(0)) / scale;
        c = R_pow_di(10.0, (1.0 - (r.log10()).floor()) as c_int);
        mm = imin2(2, imax2(0, (r * c / 25.0) as c_int));
        k = 3 * mm + 2 - 150 / (n + 50);
        if (k - 1) * (k - 2) * (k - 5) == 0 {
            c *= 10.0;
        }
        /* need to ensure that x[i]*c does not integer overflow */
        let mut x1 = (*x.add(0)).abs();
        let mut x2 = (*x.add((n - 1) as usize)).abs();
        if x2 > x1 {
            x1 = x2;
        }
        while x1 * c > c_int::MAX as f64 {
            c /= 10.0;
        }
        if k * (k - 4) * (k - 8) == 0 {
            mu = 5.0;
        }
        if (k - 1) * (k - 5) * (k - 6) == 0 {
            mu = 20.0;
        }
    } else {
        r = atom + (*x.add(0)).abs() / scale;
        c = R_pow_di(10.0, (1.0 - (r.log10()).floor()) as c_int);
    }

    /* Find the print width of the stem. */
    let xlow = rnd(*x.add(0), c);
    let xhigh = rnd(*x.add((n - 1) as usize), c);
    let lo_nd = (xlow / mu).floor() * mu;
    let hi_nd = (xhigh / mu).floor() * mu;

    ldigits = if lo_nd < 0.0 {
        ((-lo_nd).log10()).floor() as c_int + 1
    } else {
        0
    };
    hdigits = if hi_nd > 0.0 {
        (hi_nd.log10()).floor() as c_int
    } else {
        0
    };
    ndigits = if ldigits < hdigits { hdigits } else { ldigits };

    /* Starting cell */
    lo = (*x.add(0) * c / mu).floor() * mu;
    hi = (*x.add((n - 1) as usize) * c / mu).floor() * mu;

    if lo < 0.0 && (*x.add(0) * c).floor() == lo {
        lo = lo - mu;
    }
    hi = lo + mu;
    if (*x.add(0) * c + 0.5).floor() > hi {
        lo = hi;
        hi = lo + mu;
    }

    /* Print out the info about the decimal place */
    pdigits = 1 - ((c.log10() + 0.5).floor()) as c_int;

    Rprintf("  The decimal point is ");
    if pdigits == 0 {
        Rprintf("at the |\n\n");
    } else {
        if pdigits > 0 {
            Rprintf(&format!("{} digit(s) to the right of the |\n\n", pdigits));
        } else {
            Rprintf(&format!(
                "{} digit(s) to the left of the |\n\n",
                pdigits.abs()
            ));
        }
    }

    i = 0;
    loop {
        if lo < 0.0 {
            stem_print(hi as c_int, lo as c_int, ndigits);
        } else {
            stem_print(lo as c_int, hi as c_int, ndigits);
        }
        j = 0;
        loop {
            xi = rnd(*x.add(i as usize), c) as c_int;

            if (hi == 0.0 && *x.add(i as usize) >= 0.0)
                || (lo < 0.0 && xi > hi as c_int)
                || (lo >= 0.0 && xi >= hi as c_int)
            {
                break;
            }

            j += 1;
            if j <= width - 12 {
                Rprintf(&format!("{}", xi.abs() % 10));
            }
            i += 1;
            if i >= n {
                break;
            }
        }
        if j > width {
            Rprintf(&format!("+{}", j - width));
        }
        Rprintf("\n");
        if i >= n {
            break;
        }
        hi += mu;
        lo += mu;
    }
    Rprintf("\n");
}

/* The R wrapper has removed NAs from x */
pub unsafe fn C_StemLeaf(x: SEXP, scale: SEXP, swidth: SEXP, atom: SEXP) -> SEXP {
    use crate::main::errors::Rf_error;

    if TYPEOF(x) != SEXPTYPE::REALSXP || TYPEOF(scale) != SEXPTYPE::REALSXP {
        Rf_error(b"invalid input\0".as_ptr() as *const libc::c_char);
    }
    let width = asInteger(swidth);
    let n = LENGTH(x);
    if n == NA_INTEGER {
        Rf_error(b"invalid 'x' argument\0".as_ptr() as *const libc::c_char);
    }
    if width == NA_INTEGER {
        Rf_error(b"invalid 'width' argument\0".as_ptr() as *const libc::c_char);
    }
    let sc = asReal(scale);
    let sa = asReal(atom);
    if !R_FINITE(sc) {
        Rf_error(b"invalid 'scale' argument\0".as_ptr() as *const libc::c_char);
    }
    if !R_FINITE(sa) {
        Rf_error(b"invalid 'atom' argument\0".as_ptr() as *const libc::c_char);
    }

    /* Make a mutable copy since R_rsort sorts in place */
    let xcopy = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, n));
    std::ptr::copy_nonoverlapping(REAL(x), REAL(xcopy), n as usize);
    stem_leaf(REAL(xcopy), n, sc, width, sa);

    Rf_unprotect(1);
    R_NilValue()
}

unsafe fn C_bincount(
    x: *const c_double,
    n: R_xlen_t,
    breaks: *const c_double,
    nb: R_xlen_t,
    count: *mut c_int,
    right: c_int,
    include_border: c_int,
) {
    let nb1 = nb - 1;

    /* zero out count array */
    for i in 0..(nb1 as usize) {
        *count.add(i) = 0;
    }

    for i in 0..(n as usize) {
        if R_FINITE(*x.add(i)) {
            let mut lo: R_xlen_t = 0;
            let mut hi: R_xlen_t = nb1;
            if *breaks.add(lo as usize) <= *x.add(i)
                && (*x.add(i) < *breaks.add(hi as usize)
                    || (*x.add(i) == *breaks.add(hi as usize) && include_border != 0))
            {
                while hi - lo >= 2 {
                    let new = (hi + lo) / 2;
                    if *x.add(i) > *breaks.add(new as usize)
                        || (right == 0 && *x.add(i) == *breaks.add(new as usize))
                    {
                        lo = new;
                    } else {
                        hi = new;
                    }
                }
                *count.add(lo as usize) += 1;
            }
        }
    }
}

/* The R wrapper removed non-finite values */
pub unsafe fn C_BinCount(x: SEXP, breaks: SEXP, right: SEXP, lowest: SEXP) -> SEXP {
    use crate::main::errors::Rf_error;

    let x = coerceVector(x, SEXPTYPE::REALSXP.into());
    Rf_protect(x);
    let breaks = coerceVector(breaks, SEXPTYPE::REALSXP.into());
    Rf_protect(breaks);

    let n = XLENGTH(x);
    let nB = XLENGTH(breaks);
    let sr = asLogical(right);
    let sl = asLogical(lowest);
    if sr == NA_INTEGER {
        Rf_error(b"invalid 'right' argument\0".as_ptr() as *const libc::c_char);
    }
    if sl == NA_INTEGER {
        Rf_error(b"invalid 'include.lowest' argument\0".as_ptr() as *const libc::c_char);
    }

    let counts = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP, (nB - 1) as c_int));
    C_bincount(REAL(x), n, REAL(breaks), nB, INTEGER(counts), sr, sl);

    Rf_unprotect(3);
    counts
}
