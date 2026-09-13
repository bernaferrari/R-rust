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
    unsafe { crate::main::coerce::coerceVector(x, type_) }
}

unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe { crate::main::coerce::asInteger(x) }
}

unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe { crate::main::coerce::asLogical(x) }
}

unsafe fn asBool(x: SEXP) -> bool {
    unsafe {
        let v = asLogical(x);
        v != 0 && v != NA_INTEGER
    }
}

unsafe fn as_integer(x: SEXP) -> c_int {
    unsafe {
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
}

unsafe fn as_logical(x: SEXP) -> c_int {
    unsafe {
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
}

fn my_isok(x: c_double) -> bool {
    !ISNAN(x)
}

unsafe fn nrows(x: SEXP) -> c_int {
    unsafe {
        let d = getAttrib(x, R_DimSymbol());
        if d.is_null() || d == R_NilValue() {
            return LENGTH(x);
        }
        *INTEGER(d)
    }
}

unsafe fn ncols(x: SEXP) -> c_int {
    unsafe {
        let d = getAttrib(x, R_DimSymbol());
        if d.is_null() || d == R_NilValue() {
            return 1;
        }
        if LENGTH(d) >= 2 {
            return *INTEGER(d).add(1);
        }
        1
    }
}

pub unsafe fn cfilter(sx: SEXP, sfilter: SEXP, ssides: SEXP, scircular: SEXP) -> SEXP {
    unsafe {
        use crate::main::errors::Rf_error;

        if TYPEOF(sx) != SEXPTYPE::REALSXP || TYPEOF(sfilter) != SEXPTYPE::REALSXP {
            Rf_error(b"invalid input\0".as_ptr() as *const core::ffi::c_char);
        }

        let nx = XLENGTH(sx) as isize;
        let nf = XLENGTH(sfilter) as isize;
        let sides = as_integer(ssides);
        let circular = as_logical(scircular);
        if sides == NA_INTEGER || circular == NA_INTEGER {
            Rf_error(b"invalid input\0".as_ptr() as *const core::ffi::c_char);
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
}

/// GNU `filter(x, filter)` convolution, sides=2.
pub unsafe fn do_filter(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let f = CAR(CDR(args));
        let xd = if TYPEOF(x) == SEXPTYPE::REALSXP {
            x
        } else {
            coerceVector(x, SEXPTYPE::REALSXP.as_c_int())
        };
        let _xd = protect(xd);
        let fd = if TYPEOF(f) == SEXPTYPE::REALSXP {
            f
        } else {
            coerceVector(f, SEXPTYPE::REALSXP.as_c_int())
        };
        let _fd = protect(fd);
        let sides = Rf_ScalarInteger(2);
        let _s = protect(sides);
        let circ = Rf_ScalarLogical(0);
        let _c = protect(circ);
        cfilter(xd, fd, sides, circ)
    }
}


/* recursive filtering */
pub unsafe fn rfilter(x: SEXP, filter: SEXP, out: SEXP) -> SEXP {
    unsafe {
        use crate::main::errors::Rf_error;

        if TYPEOF(x) != SEXPTYPE::REALSXP
            || TYPEOF(filter) != SEXPTYPE::REALSXP
            || TYPEOF(out) != SEXPTYPE::REALSXP
        {
            Rf_error(b"invalid input\0".as_ptr() as *const core::ffi::c_char);
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
    unsafe {
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
                            let a = *acf.add(lag + (d1 as usize) * u + (d2 as usize) * v)
                                / (se[u] * se[v]);
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
}

pub unsafe fn acf(x: SEXP, lmax: SEXP, sCor: SEXP) -> SEXP {
    unsafe {
        let nx = nrows(x);
        let ns = ncols(x);
        let lagmax = as_integer(lmax);
        let cor = as_logical(sCor) != 0;
        let x = coerceVector(x, SEXPTYPE::REALSXP.as_c_int());
        let _x_guard = protect(x);

        let ans_size = (lagmax as isize + 1) * ns as isize * ns as isize;
        let ans = Rf_allocVector(SEXPTYPE::REALSXP, ans_size as c_int);
        let _ans_guard = protect(ans);
        acf0(REAL(x), nx, ns, lagmax, cor, REAL(ans));

        let d = Rf_allocVector(SEXPTYPE::INTSXP, 3);
        let _d_guard = protect(d);
        *INTEGER(d) = lagmax + 1;
        *INTEGER(d).add(1) = ns;
        *INTEGER(d).add(2) = ns;
        setAttrib(ans, R_DimSymbol(), d);

        ans
    }
}

pub unsafe fn do_acf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x0 = CAR(args);
        let mut lagmax = 10;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "lag.max" {
                let v = CAR(cell);
                lagmax = if TYPEOF(v) == SEXPTYPE::INTSXP {
                    *INTEGER(v)
                } else {
                    *REAL(v) as c_int
                };
            }
            cell = CDR(cell);
        }
        let n = XLENGTH(x0);
        let xd = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _xd = protect(xd);
        let mut mean = 0.0;
        for i in 0..n {
            let v = if TYPEOF(x0) == SEXPTYPE::REALSXP {
                *REAL(x0).add(i as usize)
            } else {
                *INTEGER(x0).add(i as usize) as f64
            };
            *REAL(xd).add(i as usize) = v;
            mean += v;
        }
        mean /= n as f64;
        for i in 0..n {
            *REAL(xd).add(i as usize) -= mean;
        }
        let lmax = Rf_ScalarInteger(lagmax);
        let _l = protect(lmax);
        let scor = Rf_ScalarLogical(1);
        let _c = protect(scor);
        let a = acf(xd, lmax, scor);
        let _a = protect(a);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, a);
        crate::mainutils::essentials::set_string_names(result, &["acf".to_string()]);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"acf".as_ptr()),
        );
        result
    }
}

/// GNU `pacf` via Durbin-Levinson on demeaned acf.
pub unsafe fn do_pacf(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let a = do_acf(_call, _op, args, rho);
        let _a = protect(a);
        let acfv = VECTOR_ELT(a, 0);
        let nlag = XLENGTH(acfv) as usize;
        if nlag < 2 {
            return a;
        }
        let m = nlag - 1;
        let mut rho_v = vec![0.0f64; nlag];
        for i in 0..nlag {
            rho_v[i] = *REAL(acfv).add(i);
        }
        let pac = Rf_allocVector3(SEXPTYPE::REALSXP, m as i64);
        let _p = protect(pac);
        let mut phi_prev = vec![0.0f64; m];
        let mut phi = vec![0.0f64; m];
        for k in 1..=m {
            let mut num = rho_v[k];
            let mut den = 1.0;
            for j in 1..k {
                num -= phi_prev[j - 1] * rho_v[k - j];
                den -= phi_prev[j - 1] * rho_v[j];
            }
            let phikk = if den.abs() > 1e-15 { num / den } else { 0.0 };
            phi[k - 1] = phikk;
            for j in 1..k {
                phi[j - 1] = phi_prev[j - 1] - phikk * phi_prev[k - j - 1];
            }
            *REAL(pac).add(k - 1) = phikk;
            phi_prev.clone_from(&phi);
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, pac);
        crate::mainutils::essentials::set_string_names(result, &["acf".to_string()]);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"acf".as_ptr()),
        );
        result
    }
}


/// GNU `ccf(x, y, lag.max)` demeaned cross-correlation.
pub unsafe fn do_ccf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x0 = CAR(args);
        let y0 = CAR(CDR(args));
        let mut lagmax = 2;
        let mut cell = CDR(CDR(args));
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "lag.max" {
                let v = CAR(cell);
                lagmax = if TYPEOF(v) == SEXPTYPE::INTSXP {
                    *INTEGER(v)
                } else {
                    *REAL(v) as c_int
                };
            }
            cell = CDR(cell);
        }
        let n = XLENGTH(x0) as usize;
        let mut x = vec![0.0f64; n];
        let mut y = vec![0.0f64; n];
        let mut mx = 0.0;
        let mut my = 0.0;
        for i in 0..n {
            x[i] = if TYPEOF(x0) == SEXPTYPE::REALSXP {
                *REAL(x0).add(i)
            } else {
                *INTEGER(x0).add(i) as f64
            };
            y[i] = if TYPEOF(y0) == SEXPTYPE::REALSXP {
                *REAL(y0).add(i)
            } else {
                *INTEGER(y0).add(i) as f64
            };
            mx += x[i];
            my += y[i];
        }
        mx /= n as f64;
        my /= n as f64;
        for i in 0..n {
            x[i] -= mx;
            y[i] -= my;
        }
        let out_n = (2 * lagmax + 1) as usize;
        let acfv = Rf_allocVector3(SEXPTYPE::REALSXP, out_n as i64);
        let _ac = protect(acfv);
        for (idx, lag) in (-lagmax..=lagmax).enumerate() {
            let mut sum = 0.0;
            if lag >= 0 {
                let l = lag as usize;
                for i in 0..(n - l) {
                    sum += x[i] * y[i + l];
                }
            } else {
                let l = (-lag) as usize;
                for i in 0..(n - l) {
                    sum += x[i + l] * y[i];
                }
            }
            *REAL(acfv).add(idx) = sum / (n as f64);
        }
        // scale to correlation by lag-0 variances
        let mut c0 = 0.0;
        for i in 0..n {
            c0 += x[i] * x[i];
        }
        let mut d0 = 0.0;
        for i in 0..n {
            d0 += y[i] * y[i];
        }
        let scale = (c0 * d0).sqrt() / (n as f64);
        if scale > 0.0 {
            for i in 0..out_n {
                *REAL(acfv).add(i) /= scale;
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, acfv);
        crate::mainutils::essentials::set_string_names(result, &["acf".to_string()]);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"acf".as_ptr()),
        );
        result
    }
}


