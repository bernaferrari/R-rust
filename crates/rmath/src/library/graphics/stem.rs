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

use crate::main::coerce::{asInteger, asLogical, asReal, coerceVector};
use crate::main::sort::rsort_slice;

fn Rprintf(msg: &str) -> c_int {
    crate::sexp::output::capture_stdout(msg);
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

fn stem_print(close: c_int, dist: c_int, ndigits: c_int) {
    if close / 10 == 0 && (close < 0 || dist < 0) {
        Rprintf(&format!("  {:1$} | ", "-0", ndigits as usize));
    } else {
        Rprintf(&format!("  {:1$} | ", close / 10, ndigits as usize));
    }
}

fn rnd(u: c_double, c: c_double) -> c_double {
    if u < 0.0 { u * c - 0.5 } else { u * c + 0.5 }
}

fn stem_leaf(x: &mut [c_double], scale: c_double, width: c_int, atom: c_double) {
    {
        let n = x.len() as c_int;
        let mut r: c_double;
        let mut c: c_double;
        let mut mu: c_double = 10.0_f64;
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

        rsort_slice(x);

        Rprintf("\n");
        if x[(n - 1) as usize] > x[0] {
            r = atom + (x[(n - 1) as usize] - x[0]) / scale;
            c = R_pow_di(10.0, (1.0 - (r.log10()).floor()) as c_int);
            mm = imin2(2, imax2(0, (r * c / 25.0) as c_int));
            k = 3 * mm + 2 - 150 / (n + 50);
            if (k - 1) * (k - 2) * (k - 5) == 0 {
                c *= 10.0;
            }
            /* need to ensure that x[i]*c does not integer overflow */
            let mut x1 = (x[0]).abs();
            let mut x2 = (x[(n - 1) as usize]).abs();
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
            r = atom + (x[0]).abs() / scale;
            c = R_pow_di(10.0, (1.0 - (r.log10()).floor()) as c_int);
        }

        /* Find the print width of the stem. */
        let xlow = rnd(x[0], c);
        let xhigh = rnd(x[(n - 1) as usize], c);
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
        lo = (x[0] * c / mu).floor() * mu;
        hi = (x[(n - 1) as usize] * c / mu).floor() * mu;

        if lo < 0.0 && (x[0] * c).floor() == lo {
            lo = lo - mu;
        }
        hi = lo + mu;
        if (x[0] * c + 0.5).floor() > hi {
            lo = hi;
            hi = lo + mu;
        }

        /* Print out the info about the decimal place */
        pdigits = 1 - ((c.log10() + 0.5).floor()) as c_int;

        Rprintf("  The decimal point is ");
        if pdigits == 0 {
            Rprintf("at the |\n\n");
        } else if pdigits > 0 {
            Rprintf(&format!("{} digit(s) to the right of the |\n\n", pdigits));
        } else {
            Rprintf(&format!(
                "{} digit(s) to the left of the |\n\n",
                pdigits.abs()
            ));
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
                xi = rnd(x[i as usize], c) as c_int;

                if (hi == 0.0 && x[i as usize] >= 0.0)
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
}

/* The R wrapper has removed NAs from x */
pub fn C_StemLeaf(x: SEXP, scale: SEXP, swidth: SEXP, atom: SEXP) -> SEXP {
    use crate::main::errors::{Rf_error, Rf_error_cstr};

    {
        if TYPEOF(x) != SEXPTYPE::REALSXP || TYPEOF(scale) != SEXPTYPE::REALSXP {
            Rf_error_cstr(c"invalid input");
        }
        let width = asInteger(swidth);
        let n = LENGTH(x);
        if n == NA_INTEGER {
            Rf_error_cstr(c"invalid 'x' argument");
        }
        if width == NA_INTEGER {
            Rf_error_cstr(c"invalid 'width' argument");
        }
        let sc = asReal(scale);
        let sa = asReal(atom);
        if !R_FINITE(sc) {
            Rf_error_cstr(c"invalid 'scale' argument");
        }
        if !R_FINITE(sa) {
            Rf_error_cstr(c"invalid 'atom' argument");
        }

        /* Make a mutable copy since R_rsort sorts in place */
        let xcopy = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let _xcopy_guard = protect(xcopy);
        // SAFETY: `x` is a live REALSXP of length n and `xcopy` is a freshly
        // allocated REALSXP of the same length, both protected here.
        let xcopy_data = unsafe {
            std::ptr::copy_nonoverlapping(REAL(x), REAL(xcopy), n as usize);
            std::slice::from_raw_parts_mut(REAL(xcopy), n as usize)
        };
        stem_leaf(xcopy_data, sc, width, sa);

        R_NilValue()
    }
}

fn C_bincount(
    x: &[c_double],
    breaks: &[c_double],
    count: &mut [c_int],
    right: c_int,
    include_border: c_int,
) {
    {
        let n = x.len() as R_xlen_t;
        let nb = breaks.len() as R_xlen_t;
        let nb1 = nb - 1;

        /* zero out count array */
        for i in 0..(nb1 as usize) {
            count[i] = 0;
        }

        for i in 0..(n as usize) {
            if R_FINITE(x[i]) {
                let mut lo: R_xlen_t = 0;
                let mut hi: R_xlen_t = nb1;
                if breaks[lo as usize] <= x[i]
                    && (x[i] < breaks[hi as usize]
                        || (x[i] == breaks[hi as usize] && include_border != 0))
                {
                    while hi - lo >= 2 {
                        let new = (hi + lo) / 2;
                        if x[i] > breaks[new as usize]
                            || (right == 0 && x[i] == breaks[new as usize])
                        {
                            lo = new;
                        } else {
                            hi = new;
                        }
                    }
                    count[lo as usize] += 1;
                }
            }
        }
    }
}

/* The R wrapper removed non-finite values */
pub fn C_BinCount(x: SEXP, breaks: SEXP, right: SEXP, lowest: SEXP) -> SEXP {
    use crate::main::errors::{Rf_error, Rf_error_cstr};

    {
        let x = coerceVector(x, SEXPTYPE::REALSXP.into());
        let _x_guard = protect(x);
        let breaks = coerceVector(breaks, SEXPTYPE::REALSXP.into());
        let _breaks_guard = protect(breaks);

        let n = XLENGTH(x);
        let nB = XLENGTH(breaks);
        let sr = asLogical(right);
        let sl = asLogical(lowest);
        if sr == NA_INTEGER {
            Rf_error_cstr(c"invalid 'right' argument");
        }
        if sl == NA_INTEGER {
            Rf_error_cstr(c"invalid 'include.lowest' argument");
        }

        let counts = Rf_allocVector(SEXPTYPE::INTSXP, (nB - 1) as c_int);
        let _counts_guard = protect(counts);
        // SAFETY: `x`, `breaks`, and `counts` are live protected vectors of
        // the exact lengths sliced here. Empty vectors may have a null data
        // pointer, so they are lent as empty slices without touching it.
        let (x_data, breaks_data, counts_data): (&[f64], &[f64], &mut [c_int]) = unsafe {
            (
                if n > 0 {
                    std::slice::from_raw_parts(REAL(x), n as usize)
                } else {
                    &[]
                },
                if nB > 0 {
                    std::slice::from_raw_parts(REAL(breaks), nB as usize)
                } else {
                    &[]
                },
                if nB > 1 {
                    std::slice::from_raw_parts_mut(INTEGER(counts), (nB - 1) as usize)
                } else {
                    &mut []
                },
            )
        };
        C_bincount(x_data, breaks_data, counts_data, sr, sl);

        counts
    }
}

/// GNU `stem(x, scale=1, width=80, atom=1e-8)`.
pub fn do_stem(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        let x = CAR(args);
        let xt = TYPEOF(x);
        if xt != SEXPTYPE::INTSXP && xt != SEXPTYPE::REALSXP && xt != SEXPTYPE::LGLSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'x' must be numeric",
            );
        }
        let n = XLENGTH(x);
        let mut finite: Vec<f64> = Vec::new();
        for i in 0..n {
            let v = if xt == SEXPTYPE::REALSXP {
                REAL_ELT(x, i as c_int)
            } else {
                let iv = INTEGER_ELT(x, i as c_int);
                if iv == NA_INTEGER {
                    f64::NAN
                } else {
                    iv as f64
                }
            };
            if v.is_finite() {
                finite.push(v);
            }
        }
        if finite.is_empty() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "no finite and non-missing values",
            );
        }
        let mut scale = 1.0;
        let mut width = 80;
        let mut atom = 1e-8;
        let mut cell = CDR(args);
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                String::from_utf8_lossy(&charsxp_as_utf8(PRINTNAME(tag))).into_owned()
            } else {
                String::new()
            };
            let slot = match name.as_str() {
                "scale" => 0,
                "width" => 1,
                "atom" => 2,
                _ => {
                    let s = pos;
                    pos += 1;
                    s
                }
            };
            let v = CAR(cell);
            match slot {
                0 => {
                    scale = if TYPEOF(v) == SEXPTYPE::REALSXP {
                        REAL_ELT(v, 0)
                    } else {
                        INTEGER_ELT(v, 0) as f64
                    };
                }
                1 => {
                    width = if TYPEOF(v) == SEXPTYPE::INTSXP {
                        INTEGER_ELT(v, 0)
                    } else {
                        REAL_ELT(v, 0) as c_int
                    };
                }
                2 => {
                    atom = if TYPEOF(v) == SEXPTYPE::REALSXP {
                        REAL_ELT(v, 0)
                    } else {
                        INTEGER_ELT(v, 0) as f64
                    };
                }
                _ => {}
            }
            cell = CDR(cell);
        }
        if scale <= 0.0 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'scale' must be positive",
            );
        }
        let xd = Rf_allocVector3(SEXPTYPE::REALSXP, finite.len() as i64);
        let _xd = protect(xd);
        for (i, v) in finite.iter().enumerate() {
            SET_REAL_ELT(xd, i as c_int, *v);
        }
        let sc = Rf_ScalarReal(scale);
        let _sc = protect(sc);
        let w = Rf_ScalarInteger(width);
        let _w = protect(w);
        let a = Rf_ScalarReal(atom);
        let _a = protect(a);
        C_StemLeaf(xd, sc, w, a);
        crate::eval::runtime::set_visible(FALSE);
        R_NilValue()
    }
}
