#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1999-2025   The R Core Team
 *  Copyright (C) 1995--1997  Robert Gentleman and Ross Ihaka
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
 *  Ported from r-source/src/library/stats/src/filter.c
 */

use std::os::raw::{c_double, c_int, c_longlong};

use crate::attrib_core::{R_DimSymbol, getAttrib, setAttrib};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::main::coerce::coerceVector(x, type_)
}

unsafe fn asInteger(x: SEXP) -> c_int {
    crate::main::coerce::asInteger(x)
}

unsafe fn asLogical(x: SEXP) -> c_int {
    crate::main::coerce::asLogical(x)
}

unsafe fn asBool(x: SEXP) -> bool {
    let v = asLogical(x);
    v != 0 && v != NA_INTEGER
}

unsafe fn as_integer(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::INTSXP {
        return *INTEGER(x);
    }
    if t == SEXPTYPE::REALSXP {
        let v = *REAL(x);
        if v.is_nan() || v < c_int::MIN as f64 || v > c_int::MAX as f64 {
            return NA_INTEGER;
        }
        return v as c_int;
    }
    if t == SEXPTYPE::LGLSXP {
        return *INTEGER(x);
    }
    NA_INTEGER
}

unsafe fn as_logical(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LGLSXP {
        return *INTEGER(x);
    }
    if t == SEXPTYPE::INTSXP {
        return *INTEGER(x);
    }
    NA_INTEGER
}

fn my_isok(x: c_double) -> bool {
    !ISNAN(x)
}

unsafe fn nrows(x: SEXP) -> c_int {
    let d = getAttrib(x, R_DimSymbol());
    if d.is_null() {
        return LENGTH(x);
    }
    *INTEGER(d)
}

unsafe fn ncols(x: SEXP) -> c_int {
    let d = getAttrib(x, R_DimSymbol());
    if d.is_null() {
        return 1;
    }
    if LENGTH(d) >= 2 {
        return *INTEGER(d.add(1));
    }
    1
}

pub unsafe fn cfilter(sx: SEXP, sfilter: SEXP, ssides: SEXP, scircular: SEXP) -> SEXP {
    use crate::main::errors::Rf_error;

    if TYPEOF(sx) != SEXPTYPE::REALSXP || TYPEOF(sfilter) != SEXPTYPE::REALSXP {
        Rf_error(b"invalid input\0".as_ptr() as *const i8);
    }

    let nx = XLENGTH(sx) as isize;
    let nf = XLENGTH(sfilter) as isize;
    let sides = as_integer(ssides);
    let circular = as_logical(scircular);
    if sides == NA_INTEGER || circular == NA_INTEGER {
        Rf_error(b"invalid input\0".as_ptr() as *const i8);
    }

    let ans = Rf_allocVector(SEXPTYPE::REALSXP, nx as c_int);
    let x = REAL(sx);
    let filter = REAL(sfilter);
    let out = REAL(ans);

    let nshift = if sides == 2 { nf / 2 } else { 0 };

    if circular == 0 {
        for i in 0..(nx as usize) {
            let mut z: c_double = 0.0;
            let i_nshift = i as isize + nshift;

            if i_nshift - (nf - 1) < 0 || i_nshift >= nx {
                *out.add(i) = NA_REAL;
                continue;
            }

            let j_start = if nshift + i as isize - nx > 0 {
                (nshift + i as isize - nx) as usize
            } else {
                0usize
            };
            let j_end = if nf < i_nshift + 1 {
                nf as usize
            } else {
                (i_nshift + 1) as usize
            };

            let mut bad = false;
            let mut j = j_start;
            loop {
                if j >= j_end {
                    break;
                }
                let tmp = *x.add((i_nshift - j as isize) as usize);
                if my_isok(tmp) {
                    z += *filter.add(j) * tmp;
                } else {
                    *out.add(i) = NA_REAL;
                    bad = true;
                    break;
                }
                j += 1;
            }
            if !bad {
                *out.add(i) = z;
            }
        }
    } else {
        /* circular */
        for i in 0..(nx as usize) {
            let mut z: c_double = 0.0;
            let mut bad = false;

            for j in 0..(nf as usize) {
                let mut ii = i as isize + nshift - j as isize;
                if ii < 0 {
                    ii += nx;
                }
                if ii >= nx {
                    ii -= nx;
                }
                let tmp = *x.add(ii as usize);
                if my_isok(tmp) {
                    z += *filter.add(j) * tmp;
                } else {
                    *out.add(i) = NA_REAL;
                    bad = true;
                    break;
                }
            }
            if !bad {
                *out.add(i) = z;
            }
        }
    }

    ans
}

/* recursive filtering */
pub unsafe fn rfilter(x: SEXP, filter: SEXP, out: SEXP) -> SEXP {
    use crate::main::errors::Rf_error;

    if TYPEOF(x) != SEXPTYPE::REALSXP
        || TYPEOF(filter) != SEXPTYPE::REALSXP
        || TYPEOF(out) != SEXPTYPE::REALSXP
    {
        Rf_error(b"invalid input\0".as_ptr() as *const i8);
    }

    let nx = XLENGTH(x);
    let nf = XLENGTH(filter);
    let r = REAL(out);
    let rx = REAL(x);
    let rf = REAL(filter);

    for i in 0..(nx as usize) {
        let mut sum = *rx.add(i);
        if !my_isok(sum) {
            *r.add(nf as usize + i) = NA_REAL;
            continue;
        }
        let mut bad = false;
        for j in 0..(nf as usize) {
            let tmp = *r.add(nf as usize + i - j - 1);
            if my_isok(tmp) {
                sum += tmp * *rf.add(j);
            } else {
                *r.add(nf as usize + i) = NA_REAL;
                bad = true;
                break;
            }
        }
        if !bad {
            *r.add(nf as usize + i) = sum;
        }
    }

    out
}

/* now allows missing values */
unsafe fn acf0(
    x: *const c_double,
    n: c_int,
    ns: c_int,
    nl: c_int,
    correlation: bool,
    acf: *mut c_double,
) {
    let d1 = (nl + 1) as isize;
    let d2 = (ns * d1 as c_int) as isize;

    for u in 0..(ns as usize) {
        for v in 0..(ns as usize) {
            for lag in 0..=(nl as usize) {
                let mut sum = 0.0;
                let mut nu: c_int = 0;
                for i in 0..((n - lag as c_int) as usize) {
                    let xu = *x.add(i + lag + (n as usize) * u);
                    let xv = *x.add(i + (n as usize) * v);
                    if !ISNAN(xu) && !ISNAN(xv) {
                        nu += 1;
                        sum += xu * xv;
                    }
                }
                let val = if nu > 0 {
                    sum / (nu as c_double + lag as c_double)
                } else {
                    NA_REAL
                };
                *acf.add(lag + (d1 as usize) * u + (d2 as usize) * v) = val;
            }
        }
    }

    if correlation {
        if n == 1 {
            for u in 0..(ns as usize) {
                *acf.add(0 + (d1 as usize) * u + (d2 as usize) * u) = 1.0;
            }
        } else {
            let mut se = vec![0.0f64; ns as usize];
            for u in 0..(ns as usize) {
                se[u] = (*acf.add(0 + (d1 as usize) * u + (d2 as usize) * u)).sqrt();
            }
            for u in 0..(ns as usize) {
                for v in 0..(ns as usize) {
                    for lag in 0..=(nl as usize) {
                        let a =
                            *acf.add(lag + (d1 as usize) * u + (d2 as usize) * v) / (se[u] * se[v]);
                        let clamped = if a > 1.0 {
                            1.0
                        } else if a < -1.0 {
                            -1.0
                        } else {
                            a
                        };
                        *acf.add(lag + (d1 as usize) * u + (d2 as usize) * v) = clamped;
                    }
                }
            }
        }
    }
}

pub unsafe fn acf(x: SEXP, lmax: SEXP, sCor: SEXP) -> SEXP {
    let nx = nrows(x);
    let ns = ncols(x);
    let lagmax = as_integer(lmax);
    let cor = as_logical(sCor) != 0;
    let x = coerceVector(x, SEXPTYPE::REALSXP.as_c_int());
    Rf_protect(x);

    let ans_size = (lagmax as isize + 1) * ns as isize * ns as isize;
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, ans_size as c_int));
    acf0(REAL(x), nx, ns, lagmax, cor, REAL(ans));

    let d = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP, 3));
    *INTEGER(d) = lagmax + 1;
    *INTEGER(d.add(1)) = ns;
    *INTEGER(d.add(2)) = ns;
    setAttrib(ans, R_DimSymbol(), d);

    Rf_unprotect(3);
    ans
}
