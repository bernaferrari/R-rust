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


/// GNU `ar(..., aic=FALSE, order.max=1)` Yule-Walker.
pub unsafe fn do_ar(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let a = do_acf(_call, _op, args, rho);
        let _a = protect(a);
        let acfv = VECTOR_ELT(a, 0);
        let phi = if XLENGTH(acfv) >= 2 {
            *REAL(acfv).add(1)
        } else {
            0.0
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(phi));
        SET_VECTOR_ELT(result, 1, Rf_ScalarInteger(1));
        crate::mainutils::essentials::set_string_names(
            result,
            &["ar".to_string(), "order".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"ar".as_ptr()),
        );
        result
    }
}

/// GNU `ar.burg(..., aic=FALSE, order.max=1)` — Burg AR(1).
pub unsafe fn do_ar_burg(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x) as usize;
        if n < 2 {
            return R_NilValue();
        }
        let mut y = vec![0.0; n];
        let mut mean = 0.0;
        for i in 0..n {
            y[i] = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else {
                *INTEGER(x).add(i) as f64
            };
            mean += y[i];
        }
        mean /= n as f64;
        for yi in &mut y {
            *yi -= mean;
        }
        let mut num = 0.0;
        let mut den = 0.0;
        for t in 0..n - 1 {
            num += 2.0 * y[t + 1] * y[t];
            den += y[t + 1] * y[t + 1] + y[t] * y[t];
        }
        let phi = if den > 0.0 { num / den } else { 0.0 };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(phi));
        SET_VECTOR_ELT(result, 1, Rf_ScalarInteger(1));
        crate::mainutils::essentials::set_string_names(
            result,
            &["ar".to_string(), "order".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"ar".as_ptr()),
        );
        result
    }
}

/// GNU `ar.ols(..., aic=FALSE, order.max=1)` — OLS AR(1).
pub unsafe fn do_ar_ols(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x) as usize;
        if n < 2 {
            return R_NilValue();
        }
        let mut y = vec![0.0; n];
        let mut mean = 0.0;
        for i in 0..n {
            y[i] = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else {
                *INTEGER(x).add(i) as f64
            };
            mean += y[i];
        }
        mean /= n as f64;
        for yi in &mut y {
            *yi -= mean;
        }
        let mut num = 0.0;
        let mut den = 0.0;
        for t in 0..n - 1 {
            num += y[t + 1] * y[t];
            den += y[t] * y[t];
        }
        let phi = if den > 0.0 { num / den } else { 0.0 };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(phi));
        SET_VECTOR_ELT(result, 1, Rf_ScalarInteger(1));
        crate::mainutils::essentials::set_string_names(
            result,
            &["ar".to_string(), "order".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"ar".as_ptr()),
        );
        result
    }
}


/// Exact AR(1) Gaussian ML: SSE(φ,μ)=Σ_{t≥2}(x_t−μ−φ(x_{t−1}−μ))²+(1−φ²)(x_1−μ)².
fn exact_ar1_ml(y: &[f64]) -> (f64, f64, f64) {
    let n = y.len();
    if n < 2 {
        return (0.0, 0.0, f64::NAN);
    }
    let n_f = n as f64;
    let profile_mu = |phi: f64| -> f64 {
        if (phi - 1.0).abs() < 1e-15 {
            return y.iter().sum::<f64>() / n_f;
        }
        let den = n_f + phi * (2.0 - n_f);
        if den.abs() < 1e-18 {
            return y.iter().sum::<f64>() / n_f;
        }
        let mut num = (1.0 + phi) * y[0];
        for t in 1..n {
            num += y[t] - phi * y[t - 1];
        }
        num / den
    };
    let sse = |phi: f64, mu: f64| -> f64 {
        let mut s = (1.0 - phi * phi) * (y[0] - mu) * (y[0] - mu);
        for t in 1..n {
            let e = y[t] - mu - phi * (y[t - 1] - mu);
            s += e * e;
        }
        s
    };
    let nll = |phi: f64| -> f64 {
        if !phi.is_finite() || phi.abs() >= 1.0 {
            return f64::INFINITY;
        }
        let mu = profile_mu(phi);
        let s = sse(phi, mu);
        if !s.is_finite() || s <= 0.0 {
            return f64::INFINITY;
        }
        n_f * (s / n_f).ln() - (1.0 - phi * phi).ln()
    };
    let mut lo = -0.999999;
    let mut hi = 0.999999;
    let gr = (5.0_f64.sqrt() - 1.0) / 2.0;
    let mut c = hi - gr * (hi - lo);
    let mut d = lo + gr * (hi - lo);
    let mut fc = nll(c);
    let mut fd = nll(d);
    for _ in 0..200 {
        if fc < fd {
            hi = d;
            d = c;
            fd = fc;
            c = hi - gr * (hi - lo);
            fc = nll(c);
        } else {
            lo = c;
            c = d;
            fc = fd;
            d = lo + gr * (hi - lo);
            fd = nll(d);
        }
    }
    let phi = 0.5 * (lo + hi);
    let mu = profile_mu(phi);
    let sigma2 = sse(phi, mu) / n_f;
    (phi, mu, sigma2)
}

/// GNU `ar.mle` — univariate exact AR(1) Gaussian ML.
pub unsafe fn do_ar_mle(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dim = getAttrib(x, R_DimSymbol());
        if !dim.is_null() && dim != R_NilValue() && XLENGTH(dim) > 0 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "MLE only implemented for univariate series",
            );
        }
        if x.is_null() || x == R_NilValue() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'x' must be numeric",
            );
        }
        if TYPEOF(x) != SEXPTYPE::REALSXP && TYPEOF(x) != SEXPTYPE::INTSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'x' must be numeric",
            );
        }
        let n = XLENGTH(x) as usize;
        let mut y = vec![0.0; n];
        for i in 0..n {
            let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else {
                let iv = *INTEGER(x).add(i);
                if iv == NA_INTEGER {
                    crate::mainutils::errors::errorcall_str(
                        crate::mainutils::errors::R_getCurrentCall(),
                        "missing values in object",
                    );
                }
                iv as f64
            };
            if v.is_nan() {
                crate::mainutils::errors::errorcall_str(
                    crate::mainutils::errors::R_getCurrentCall(),
                    "missing values in object",
                );
            }
            y[i] = v;
        }
        let mut order_max: Option<i32> = None;
        let mut a = CDR(args);
        while !a.is_null() && a != R_NilValue() {
            let tag = TAG(a);
            if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned();
                if name == "order.max" {
                    order_max = Some(asInteger(CAR(a)));
                }
            }
            a = CDR(a);
        }
        if let Some(om) = order_max {
            if om < 0 {
                crate::mainutils::errors::errorcall_str(
                    crate::mainutils::errors::R_getCurrentCall(),
                    "'order.max' must be >= 0",
                );
            }
            if om as usize >= n {
                crate::mainutils::errors::errorcall_str(
                    crate::mainutils::errors::R_getCurrentCall(),
                    "'order.max' must be < 'n.used'",
                );
            }
        }
        let (phi, mu, _sigma2) = exact_ar1_ml(&y);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarInteger(1));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(phi));
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(mu));
        SET_VECTOR_ELT(result, 3, Rf_mkString(c"MLE".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "order".to_string(),
                "ar".to_string(),
                "x.mean".to_string(),
                "method".to_string(),
            ],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"ar".as_ptr()),
        );
        result
    }
}




fn css_ar1(y: &[f64]) -> (f64, f64, f64) {
    let n = y.len();
    if n < 3 {
        return (0.0, 0.0, f64::NAN);
    }
    let mut best_phi = 0.0;
    let mut best_mu = 0.0;
    let mut best_rss = f64::INFINITY;
    for k in 0..=400 {
        let phi = -0.99 + 1.98 * (k as f64) / 400.0;
        let mut num = 0.0;
        let mut den = 0.0;
        for t in 1..n {
            num += y[t] - phi * y[t - 1];
            den += 1.0 - phi;
        }
        let mu = if den.abs() > 1e-12 {
            num / den
        } else {
            y.iter().sum::<f64>() / n as f64
        };
        let mut rss = 0.0;
        for t in 1..n {
            let e = y[t] - mu - phi * (y[t - 1] - mu);
            rss += e * e;
        }
        if rss < best_rss {
            best_rss = rss;
            best_phi = phi;
            best_mu = mu;
        }
    }
    let sigma2 = best_rss / (n - 1) as f64;
    (best_phi, best_mu, sigma2)
}

/// GNU `arima(x, order=c(1,0,0), method="CSS")` — AR(1) with intercept.
pub unsafe fn do_arima(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x) as usize;
        if n < 3 {
            return R_NilValue();
        }
        let mut y = vec![0.0; n];
        for i in 0..n {
            y[i] = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else {
                *INTEGER(x).add(i) as f64
            };
        }
        let (phi, mu, sigma2) = css_ar1(&y);
        let coef = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _c = protect(coef);
        *REAL(coef) = phi;
        *REAL(coef).add(1) = mu;
        crate::mainutils::essentials::set_string_names(
            coef,
            &["ar1".to_string(), "intercept".to_string()],
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, coef);
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(sigma2));
        crate::mainutils::essentials::set_string_names(
            result,
            &["coef".to_string(), "sigma2".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"Arima".as_ptr()),
        );
        result
    }
}

/// GNU `arima0` — same CSS AR(1) as `arima`, class `arima0`.
pub unsafe fn do_arima0(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let result = do_arima(call, op, args, rho);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"arima0".as_ptr()),
        );
        result
    }
}

/// GNU `arima0.diag(...)` — `.Defunct()`.
pub unsafe fn do_arima0_diag(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::mainutils::errors::errorcall_str(
        crate::mainutils::errors::R_getCurrentCall(),
        "'arima0.diag' is defunct.\nSee help(\"Defunct\")",
    );
}

/// GNU `plclust(...)` — `.Defunct("plot")`.
pub unsafe fn do_plclust(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::mainutils::errors::errorcall_str(
        crate::mainutils::errors::R_getCurrentCall(),
        "'plclust' is defunct.\nUse 'plot' instead.\nSee help(\"Defunct\")",
    );
}




/// GNU `arima.sim(list(ar=phi), n, n.start=)` — AR(1) via rnorm + recursive filter.
pub unsafe fn do_arima_sim(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let model = CAR(args);
        let mut phi = 0.0;
        if TYPEOF(model) == SEXPTYPE::VECSXP {
            let ar = named_list_elt(model, "ar");
            if !ar.is_null() && ar != R_NilValue() {
                phi = if TYPEOF(ar) == SEXPTYPE::REALSXP {
                    *REAL(ar)
                } else if TYPEOF(ar) == SEXPTYPE::INTSXP {
                    *INTEGER(ar) as f64
                } else {
                    0.0
                };
            }
        }
        let mut n = 0i32;
        let mut n_start = 1i32;
        let mut innov = std::ptr::null_mut();
        let mut start_innov = std::ptr::null_mut();
        let mut cell = CDR(args);
        let mut pos = 1usize;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let val = CAR(cell);
            if name == "n.start" {
                n_start = if TYPEOF(val) == SEXPTYPE::INTSXP {
                    *INTEGER(val)
                } else {
                    *REAL(val) as i32
                };
            } else if name == "innov" {
                innov = val;
            } else if name == "start.innov" {
                start_innov = val;
            } else if name == "n" || (name.is_empty() && pos == 1) {
                n = if TYPEOF(val) == SEXPTYPE::INTSXP {
                    *INTEGER(val)
                } else {
                    *REAL(val) as i32
                };
            }
            pos += 1;
            cell = CDR(cell);
        }
        if n <= 0 {
            return R_NilValue();
        }
        if n_start < 0 {
            n_start = 0;
        }
        let ntot = (n + n_start) as usize;
        let mut e = vec![0.0; ntot];
        let have_explicit = !innov.is_null() && innov != R_NilValue();
        if have_explicit {
            let ns = n_start as usize;
            if !start_innov.is_null() && start_innov != R_NilValue() {
                for i in 0..ns {
                    e[i] = if TYPEOF(start_innov) == SEXPTYPE::REALSXP {
                        *REAL(start_innov).add(i.min((XLENGTH(start_innov) as usize).saturating_sub(1)))
                    } else if TYPEOF(start_innov) == SEXPTYPE::INTSXP {
                        *INTEGER(start_innov)
                            .add(i.min((XLENGTH(start_innov) as usize).saturating_sub(1)))
                            as f64
                    } else {
                        0.0
                    };
                }
            }
            let ni = XLENGTH(innov) as usize;
            for i in 0..(n as usize) {
                let src = i.min(ni.saturating_sub(1));
                e[ns + i] = if TYPEOF(innov) == SEXPTYPE::REALSXP {
                    *REAL(innov).add(src)
                } else if TYPEOF(innov) == SEXPTYPE::INTSXP {
                    *INTEGER(innov).add(src) as f64
                } else {
                    0.0
                };
            }
        } else {
            let narg = Rf_ScalarInteger(ntot as i32);
            let _na = protect(narg);
            let rargs = Rf_cons(narg, R_NilValue());
            let _ra = protect(rargs);
            let ev = crate::library::stats::random::do_rnorm_r(call, op, rargs, rho);
            let _ev = protect(ev);
            let m = XLENGTH(ev) as usize;
            for i in 0..ntot.min(m) {
                e[i] = *REAL(ev).add(i);
            }
        }
        let m = e.len();
        let mut y = vec![0.0; m];
        if m > 0 {
            y[0] = e[0];
            for i in 1..m {
                y[i] = e[i] + phi * y[i - 1];
            }
        }
        let out_n = n as usize;
        let drop = n_start as usize;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, out_n as i64);
        let _r = protect(result);
        for i in 0..out_n {
            let src = drop + i;
            *REAL(result).add(i) = if src < m { y[src] } else { 0.0 };
        }
        result
    }
}

/// GNU `makeARIMA(phi, theta, Delta)` — AR(1), no MA/difference.
pub unsafe fn do_makeARIMA(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let phi = elt_real_or(CAR(args), 0.0);
        let z = Rf_ScalarReal(1.0);
        let _z = protect(z);
        let a = Rf_ScalarReal(0.0);
        let _a = protect(a);
        let p0 = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 1, 1);
        let _p0 = protect(p0);
        *REAL(p0) = 0.0;
        let t = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 1, 1);
        let _t = protect(t);
        *REAL(t) = phi;
        let v = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 1, 1);
        let _v = protect(v);
        *REAL(v) = 1.0;
        let h = Rf_ScalarReal(0.0);
        let _h = protect(h);
        let pn = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 1, 1);
        let _pn = protect(pn);
        let den = 1.0 - phi * phi;
        *REAL(pn) = if den > 1e-15 { 1.0 / den } else { 1.0 };
        let phi_s = Rf_ScalarReal(phi);
        let _ps = protect(phi_s);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 10);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, phi_s);
        SET_VECTOR_ELT(result, 1, R_NilValue());
        SET_VECTOR_ELT(result, 2, R_NilValue());
        SET_VECTOR_ELT(result, 3, z);
        SET_VECTOR_ELT(result, 4, a);
        SET_VECTOR_ELT(result, 5, p0);
        SET_VECTOR_ELT(result, 6, t);
        SET_VECTOR_ELT(result, 7, v);
        SET_VECTOR_ELT(result, 8, h);
        SET_VECTOR_ELT(result, 9, pn);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "phi".to_string(),
                "theta".to_string(),
                "Delta".to_string(),
                "Z".to_string(),
                "a".to_string(),
                "P".to_string(),
                "T".to_string(),
                "V".to_string(),
                "h".to_string(),
                "Pn".to_string(),
            ],
        );
        result
    }
}

fn elt_real_or(x: SEXP, default: f64) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return default;
        }
        if TYPEOF(x) == SEXPTYPE::REALSXP && XLENGTH(x) > 0 {
            return *REAL(x);
        }
        if TYPEOF(x) == SEXPTYPE::INTSXP && XLENGTH(x) > 0 {
            return *INTEGER(x) as f64;
        }
        default
    }
}

struct KalmanFit {
    lik: f64,
    s2: f64,
    resid: Vec<f64>,
    states: Vec<f64>,
}

unsafe fn kalman_run_1d(y: SEXP, model: SEXP) -> Option<KalmanFit> {
    unsafe {
        if y.is_null() || y == R_NilValue() || model.is_null() || model == R_NilValue() {
            return None;
        }
        let n = XLENGTH(y) as usize;
        if n == 0 {
            return None;
        }
        let z = elt_real_or(named_list_elt(model, "Z"), 1.0);
        let mut a = elt_real_or(named_list_elt(model, "a"), 0.0);
        let t = elt_real_or(named_list_elt(model, "T"), 0.0);
        let v = elt_real_or(named_list_elt(model, "V"), 1.0);
        let h = elt_real_or(named_list_elt(model, "h"), 0.0);
        let mut p = elt_real_or(named_list_elt(model, "Pn"), 1.0);
        let mut s2 = 0.0;
        let mut sumlog = 0.0;
        let mut resid_out = vec![0.0; n];
        let mut states = vec![0.0; n];
        for i in 0..n {
            let yi = if TYPEOF(y) == SEXPTYPE::REALSXP {
                *REAL(y).add(i)
            } else {
                *INTEGER(y).add(i) as f64
            };
            let resid = yi - z * a;
            let f = z * z * p + h;
            if f > 0.0 {
                s2 += resid * resid / f;
                sumlog += f.ln();
                resid_out[i] = resid / f.sqrt();
                let k = p * z / f;
                a += k * resid;
                p -= k * k * f;
            }
            states[i] = a;
            a = t * a;
            p = t * p * t + v;
        }
        let nf = n as f64;
        s2 /= nf;
        let lik = 0.5 * (s2.ln() + sumlog / nf);
        Some(KalmanFit {
            lik,
            s2,
            resid: resid_out,
            states,
        })
    }
}

/// GNU `KalmanLike(y, mod)` — univariate AR(1) state-space likelihood.
pub unsafe fn do_kalman_like(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let Some(fit) = kalman_run_1d(CAR(args), CAR(CDR(args))) else {
            return R_NilValue();
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(fit.lik));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(fit.s2));
        crate::mainutils::essentials::set_string_names(
            result,
            &["Lik".to_string(), "s2".to_string()],
        );
        result
    }
}

/// GNU `KalmanRun(y, mod)` — likelihood, standardized residuals, filtered states.
pub unsafe fn do_kalman_run(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let Some(fit) = kalman_run_1d(CAR(args), CAR(CDR(args))) else {
            return R_NilValue();
        };
        let n = fit.resid.len();
        let values = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _v = protect(values);
        *REAL(values) = fit.lik;
        *REAL(values).add(1) = fit.s2;
        crate::mainutils::essentials::set_string_names(
            values,
            &["Lik".to_string(), "s2".to_string()],
        );
        let resid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _e = protect(resid);
        let states = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _s = protect(states);
        for i in 0..n {
            *REAL(resid).add(i) = fit.resid[i];
            *REAL(states).add(i) = fit.states[i];
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, values);
        SET_VECTOR_ELT(result, 1, resid);
        SET_VECTOR_ELT(result, 2, states);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "values".to_string(),
                "resid".to_string(),
                "states".to_string(),
            ],
        );
        result
    }
}

/// GNU `KalmanForecast(n.ahead, mod)` — iterate predict from `a` and `P`.
pub unsafe fn do_kalman_forecast(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_ahead = elt_real_or(CAR(args), 0.0).round() as usize;
        let model = CAR(CDR(args));
        if n_ahead == 0 || model.is_null() || model == R_NilValue() {
            return R_NilValue();
        }
        let z = elt_real_or(named_list_elt(model, "Z"), 1.0);
        let mut a = elt_real_or(named_list_elt(model, "a"), 0.0);
        let t = elt_real_or(named_list_elt(model, "T"), 0.0);
        let v = elt_real_or(named_list_elt(model, "V"), 1.0);
        let h = elt_real_or(named_list_elt(model, "h"), 0.0);
        let mut p = elt_real_or(named_list_elt(model, "P"), 0.0);
        let pred = Rf_allocVector3(SEXPTYPE::REALSXP, n_ahead as i64);
        let _p = protect(pred);
        let var = Rf_allocVector3(SEXPTYPE::REALSXP, n_ahead as i64);
        let _va = protect(var);
        for i in 0..n_ahead {
            a = t * a;
            p = t * p * t + v;
            *REAL(pred).add(i) = z * a;
            *REAL(var).add(i) = z * z * p + h;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, pred);
        SET_VECTOR_ELT(result, 1, var);
        crate::mainutils::essentials::set_string_names(
            result,
            &["pred".to_string(), "var".to_string()],
        );
        result
    }
}

/// GNU `KalmanSmooth(y, mod)` — Rauch–Tung–Striebel smoother, state dim 1.
pub unsafe fn do_kalman_smooth(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let y = CAR(args);
        let model = CAR(CDR(args));
        if y.is_null() || y == R_NilValue() || model.is_null() || model == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(y) as usize;
        if n == 0 {
            return R_NilValue();
        }
        let z = elt_real_or(named_list_elt(model, "Z"), 1.0);
        let mut a = elt_real_or(named_list_elt(model, "a"), 0.0);
        let t = elt_real_or(named_list_elt(model, "T"), 0.0);
        let v = elt_real_or(named_list_elt(model, "V"), 1.0);
        let h = elt_real_or(named_list_elt(model, "h"), 0.0);
        let mut p = elt_real_or(named_list_elt(model, "Pn"), 1.0);
        let mut a_pred = vec![0.0; n];
        let mut p_pred = vec![0.0; n];
        let mut a_filt = vec![0.0; n];
        let mut p_filt = vec![0.0; n];
        for i in 0..n {
            a_pred[i] = a;
            p_pred[i] = p;
            let yi = if TYPEOF(y) == SEXPTYPE::REALSXP {
                *REAL(y).add(i)
            } else {
                *INTEGER(y).add(i) as f64
            };
            let resid = yi - z * a;
            let f = z * z * p + h;
            if f > 0.0 {
                let k = p * z / f;
                a += k * resid;
                p -= k * k * f;
            }
            a_filt[i] = a;
            p_filt[i] = p.max(0.0);
            a = t * a;
            p = t * p * t + v;
        }
        let mut smooth = vec![0.0; n];
        let mut svar = vec![0.0; n];
        smooth[n - 1] = a_filt[n - 1];
        svar[n - 1] = p_filt[n - 1];
        if n >= 2 {
            for i in (0..n - 1).rev() {
                let j = if p_pred[i + 1] > 1e-15 {
                    p_filt[i] * t / p_pred[i + 1]
                } else {
                    0.0
                };
                smooth[i] = a_filt[i] + j * (smooth[i + 1] - a_pred[i + 1]);
                svar[i] = p_filt[i] + j * j * (svar[i + 1] - p_pred[i + 1]);
            }
        }
        let sm = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _sm = protect(sm);
        let va = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _va = protect(va);
        for i in 0..n {
            *REAL(sm).add(i) = smooth[i];
            *REAL(va).add(i) = svar[i].max(0.0);
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, sm);
        SET_VECTOR_ELT(result, 1, va);
        crate::mainutils::essentials::set_string_names(
            result,
            &["smooth".to_string(), "var".to_string()],
        );
        result
    }
}

fn local_level_lik(y: &[f64], q: f64, r: f64) -> (f64, f64) {
    if q < 0.0 || r < 0.0 || (q == 0.0 && r == 0.0) {
        return (f64::INFINITY, f64::NAN);
    }
    let n = y.len();
    let mut a = y[0];
    let mut p = 1e7;
    let mut s2 = 0.0;
    let mut sumlog = 0.0;
    let mut nlik = 0.0;
    for (i, &yi) in y.iter().enumerate() {
        let resid = yi - a;
        let f = p + r;
        if f > 0.0 {
            if i > 0 {
                s2 += resid * resid / f;
                sumlog += f.ln();
                nlik += 1.0;
            }
            let k = p / f;
            a += k * resid;
            p -= k * k * f;
        }
        p += q;
    }
    if nlik < 1.0 || s2 <= 0.0 {
        return (f64::INFINITY, f64::NAN);
    }
    let s2m = s2 / nlik;
    (0.5 * (s2m.ln() + sumlog / nlik), s2m)
}

fn struct_ts_type(args: SEXP) -> String {
    unsafe {
        let mut a = CDR(args);
        let mut pos = 1;
        while !a.is_null() && a != R_NilValue() {
            let tag = TAG(a);
            let named = if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if named == "type" || (named.is_empty() && pos == 1) {
                let v = CAR(a);
                if !v.is_null() && TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                    return std::ffi::CStr::from_ptr(CHAR(STRING_ELT(v, 0)))
                        .to_string_lossy()
                        .into_owned();
                }
            }
            pos += 1;
            a = CDR(a);
        }
        "level".to_string()
    }
}


/// GNU `KalmanLike` for state dim `p`. Matrices are column-major.
fn kalman_like_nd(
    y: &[f64],
    z: &[f64],
    a0: &[f64],
    p0: &[f64],
    t: &[f64],
    v: &[f64],
    h: f64,
    s_up: i32,
) -> f64 {
    let n = y.len();
    let p = a0.len();
    if n == 0 || p == 0 || z.len() != p || p0.len() != p * p || t.len() != p * p || v.len() != p * p
    {
        return f64::INFINITY;
    }
    let mut a = a0.to_vec();
    let mut pmat = p0.to_vec();
    let mut pnew = p0.to_vec();
    let mut anew = vec![0.0; p];
    let mut mm = vec![0.0; p * p];
    let mut m = vec![0.0; p];
    let mut ssq = 0.0;
    let mut sumlog = 0.0;
    let mut nu = 0.0;
    for l in 0..n {
        for i in 0..p {
            let mut tmp = 0.0;
            for k in 0..p {
                tmp += t[i + p * k] * a[k];
            }
            anew[i] = tmp;
        }
        if (l as i32) > s_up {
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = 0.0;
                    for k in 0..p {
                        tmp += t[i + p * k] * pmat[k + p * j];
                    }
                    mm[i + p * j] = tmp;
                }
            }
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = v[i + p * j];
                    for k in 0..p {
                        tmp += mm[i + p * k] * t[j + p * k];
                    }
                    pnew[i + p * j] = tmp;
                }
            }
        }
        let mut resid = y[l];
        for i in 0..p {
            resid -= z[i] * anew[i];
        }
        let mut gain = h;
        for i in 0..p {
            let mut tmp = 0.0;
            for j in 0..p {
                tmp += pnew[i + j * p] * z[j];
            }
            m[i] = tmp;
            gain += z[i] * tmp;
        }
        if !(gain > 0.0) {
            return f64::INFINITY;
        }
        ssq += resid * resid / gain;
        sumlog += gain.ln();
        nu += 1.0;
        for i in 0..p {
            a[i] = anew[i] + m[i] * resid / gain;
        }
        for i in 0..p {
            for j in 0..p {
                pmat[i + j * p] = pnew[i + j * p] - m[i] * m[j] / gain;
            }
        }
    }
    if nu < 1.0 {
        return f64::INFINITY;
    }
    0.5 * (ssq / nu + sumlog / nu)
}

/// GNU KalmanLike for local linear trend (p=2). `P[] <- 1e6*vx`.
fn kalman_trend_like(y: &[f64], rel: [f64; 3], vx: f64) -> f64 {
    if y.is_empty() || rel.iter().all(|v| *v <= 0.0) {
        return 1000.0;
    }
    let ql = rel[0].max(0.0) * vx;
    let qs = rel[1].max(0.0) * vx;
    let h = rel[2].max(0.0) * vx;
    let p0 = 1e6 * vx;
    // T = matrix(c(1,0,1,1), 2, 2) column-major.
    let t = [1.0, 0.0, 1.0, 1.0];
    let v = [ql, 0.0, 0.0, qs];
    let z = [1.0, 0.0];
    let a0 = [y[0], 0.0];
    let pmat = [p0, p0, p0, p0];
    kalman_like_nd(y, &z, &a0, &pmat, &t, &v, h, -1)
}

fn optimize_trend_rel(y: &[f64], vx: f64) -> [f64; 3] {
    let starts = [
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.15, 0.0],
        [0.0, 0.0, 100.0],
    ];
    let mut best_rel = [1.0, 1.0, 1.0];
    let mut best_v = f64::INFINITY;
    for start in starts {
        let mut rel = start;
        for _ in 0..8 {
            for i in 0..3 {
                let nll = |logv: f64| {
                    let mut r = rel;
                    r[i] = 10f64.powf(logv);
                    kalman_trend_like(y, r, vx)
                };
                let mut lo = -8.0;
                let mut hi = 4.0;
                let gr = (5.0_f64.sqrt() - 1.0) / 2.0;
                let mut c = hi - gr * (hi - lo);
                let mut d = lo + gr * (hi - lo);
                let mut fc = nll(c);
                let mut fd = nll(d);
                for _ in 0..60 {
                    if fc < fd {
                        hi = d;
                        d = c;
                        fd = fc;
                        c = hi - gr * (hi - lo);
                        fc = nll(c);
                    } else {
                        lo = c;
                        c = d;
                        fc = fd;
                        d = lo + gr * (hi - lo);
                        fd = nll(d);
                    }
                }
                let mut cand = rel;
                cand[i] = 10f64.powf(0.5 * (lo + hi));
                let mut zero = rel;
                zero[i] = 0.0;
                let v1 = kalman_trend_like(y, cand, vx);
                let v0 = kalman_trend_like(y, zero, vx);
                rel = if v0 <= v1 { zero } else { cand };
            }
        }
        let v = kalman_trend_like(y, rel, vx);
        if v < best_v {
            best_v = v;
            best_rel = rel;
        }
    }
    best_rel
}

fn series_frequency(x: SEXP) -> i32 {
    unsafe {
        let tsp = getAttrib(x, crate::sexp::symbol::Rf_install(c"tsp".as_ptr()));
        if !tsp.is_null()
            && tsp != R_NilValue()
            && TYPEOF(tsp) == SEXPTYPE::REALSXP
            && XLENGTH(tsp) >= 3
        {
            return (*REAL(tsp).add(2)).round() as i32;
        }
        1
    }
}

fn kalman_bsm_like(y: &[f64], rel: [f64; 4], vx: f64, nf: usize) -> f64 {
    if y.is_empty() || nf < 2 || rel.iter().all(|v| *v <= 0.0) {
        return 1000.0;
    }
    let p = nf + 1;
    let mut t = vec![0.0; p * p];
    t[0] = 1.0;
    t[p] = 1.0;
    t[1 + p] = 1.0;
    for j in 2..p {
        t[2 + p * j] = -1.0;
    }
    if nf >= 3 {
        for k in 2..nf {
            t[(k + 1) + p * k] = 1.0;
        }
    }
    let mut z = vec![0.0; p];
    z[0] = 1.0;
    z[2] = 1.0;
    let mut a0 = vec![0.0; p];
    a0[0] = y[0];
    let p0v = 1e6 * vx;
    let pmat = vec![p0v; p * p];
    let mut v = vec![0.0; p * p];
    v[0] = rel[0].max(0.0) * vx;
    v[1 + p] = rel[1].max(0.0) * vx;
    v[2 + 2 * p] = rel[2].max(0.0) * vx;
    let h = rel[3].max(0.0) * vx;
    kalman_like_nd(y, &z, &a0, &pmat, &t, &v, h, -1)
}

fn optimize_bsm_rel(y: &[f64], vx: f64, nf: usize) -> [f64; 4] {
    let starts = [
        [1.0, 1.0, 1.0, 1.0],
        [1.0, 0.0, 1.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 0.0],
    ];
    let mut best_rel = [1.0, 1.0, 1.0, 1.0];
    let mut best_v = f64::INFINITY;
    for start in starts {
        let mut rel = start;
        for _ in 0..8 {
            for i in 0..4 {
                let nll = |logv: f64| {
                    let mut r = rel;
                    r[i] = 10f64.powf(logv);
                    kalman_bsm_like(y, r, vx, nf)
                };
                let mut lo = -8.0;
                let mut hi = 4.0;
                let gr = (5.0_f64.sqrt() - 1.0) / 2.0;
                let mut c = hi - gr * (hi - lo);
                let mut d = lo + gr * (hi - lo);
                let mut fc = nll(c);
                let mut fd = nll(d);
                for _ in 0..50 {
                    if fc < fd {
                        hi = d;
                        d = c;
                        fd = fc;
                        c = hi - gr * (hi - lo);
                        fc = nll(c);
                    } else {
                        lo = c;
                        c = d;
                        fc = fd;
                        d = lo + gr * (hi - lo);
                        fd = nll(d);
                    }
                }
                let mut cand = rel;
                cand[i] = 10f64.powf(0.5 * (lo + hi));
                let mut zero = rel;
                zero[i] = 0.0;
                let v1 = kalman_bsm_like(y, cand, vx, nf);
                let v0 = kalman_bsm_like(y, zero, vx, nf);
                rel = if v0 <= v1 { zero } else { cand };
            }
        }
        let v = kalman_bsm_like(y, rel, vx, nf);
        if v < best_v {
            best_v = v;
            best_rel = rel;
        }
    }
    best_rel
}



/// GNU `StructTS(x, type=)` — local-level or local-linear-trend MLE.
pub unsafe fn do_struct_ts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x) as usize;
        if n < 2 {
            return R_NilValue();
        }
        let typ = struct_ts_type(args);
        if typ == "BSM" && series_frequency(x) < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "frequency must be a positive integer >= 2 for BSM",
            );
        }
        let mut y = vec![0.0; n];
        for i in 0..n {
            y[i] = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else {
                *INTEGER(x).add(i) as f64
            };
        }
        if typ == "BSM" {
            let nf = series_frequency(x) as usize;
            let mean = y.iter().sum::<f64>() / n as f64;
            let var = y.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>()
                / (n as f64 - 1.0);
            let vx = var / 100.0;
            let rel = optimize_bsm_rel(&y, vx, nf);
            let coef = Rf_allocVector3(SEXPTYPE::REALSXP, 4);
            let _c = protect(coef);
            for i in 0..4 {
                *REAL(coef).add(i) = (rel[i] * vx).max(0.0);
            }
            crate::mainutils::essentials::set_string_names(
                coef,
                &[
                    "level".to_string(),
                    "slope".to_string(),
                    "seas".to_string(),
                    "epsilon".to_string(),
                ],
            );
            let data = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
            let _d = protect(data);
            for i in 0..n {
                *REAL(data).add(i) = y[i];
            }
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            let _r = protect(result);
            SET_VECTOR_ELT(result, 0, coef);
            SET_VECTOR_ELT(result, 1, data);
            crate::mainutils::essentials::set_string_names(
                result,
                &["coef".to_string(), "data".to_string()],
            );
            let class = Rf_mkString(c"StructTS".as_ptr());
            let _cl = protect(class);
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
            return result;
        }
        if typ == "trend" {
            let mean = y.iter().sum::<f64>() / n as f64;
            let var = y.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>()
                / (n as f64 - 1.0);
            let vx = var / 100.0;
            let rel = optimize_trend_rel(&y, vx);
            let coef = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
            let _c = protect(coef);
            *REAL(coef) = (rel[0] * vx).max(0.0);
            *REAL(coef).add(1) = (rel[1] * vx).max(0.0);
            *REAL(coef).add(2) = (rel[2] * vx).max(0.0);
            crate::mainutils::essentials::set_string_names(
                coef,
                &[
                    "level".to_string(),
                    "slope".to_string(),
                    "epsilon".to_string(),
                ],
            );
            let data = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
            let _d = protect(data);
            for i in 0..n {
                *REAL(data).add(i) = y[i];
            }
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            let _r = protect(result);
            SET_VECTOR_ELT(result, 0, coef);
            SET_VECTOR_ELT(result, 1, data);
            crate::mainutils::essentials::set_string_names(
                result,
                &["coef".to_string(), "data".to_string()],
            );
            let class = Rf_mkString(c"StructTS".as_ptr());
            let _cl = protect(class);
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
            return result;
        }
        let mean = y.iter().sum::<f64>() / n as f64;
        let vx = y.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n as f64 - 1.0);
        let mut cands = vec![0.0, 1.0, vx];
        if n >= 2 {
            let mut md = 0.0;
            let mut vd = 0.0;
            for i in 1..n {
                let d = y[i] - y[i - 1];
                md += d;
                vd += d * d;
            }
            md /= (n - 1) as f64;
            vd = vd / (n - 1) as f64 - md * md;
            cands.push(md.abs());
            cands.push(md * md);
            cands.push(vd.max(0.0));
        }
        for k in -4..=0 {
            cands.push(vx * 10f64.powi(k));
        }
        let mut best_q = 0.0;
        let mut best_r = vx;
        let mut best_lik = f64::INFINITY;
        let mut best_scale = f64::INFINITY;
        for &q in &cands {
            for &r in &cands {
                let (lik, s2m) = local_level_lik(&y, q, r);
                let scale = (s2m - 1.0).abs();
                if lik < best_lik - 1e-10 || (lik < best_lik + 1e-10 && scale < best_scale) {
                    best_lik = lik;
                    best_scale = scale;
                    best_q = q;
                    best_r = r;
                }
            }
        }
        let coef = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _c = protect(coef);
        *REAL(coef) = best_q;
        *REAL(coef).add(1) = best_r;
        crate::mainutils::essentials::set_string_names(
            coef,
            &["level".to_string(), "epsilon".to_string()],
        );
        let data = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _d = protect(data);
        for i in 0..n {
            *REAL(data).add(i) = y[i];
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, coef);
        SET_VECTOR_ELT(result, 1, data);
        crate::mainutils::essentials::set_string_names(
            result,
            &["coef".to_string(), "data".to_string()],
        );
        let class = Rf_mkString(c"StructTS".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `tsSmooth(StructTS)` — Kalman smooth of the fitted local level.
pub unsafe fn do_ts_smooth(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let data = named_list_elt(obj, "data");
        let coef = named_list_elt(obj, "coef");
        if data == R_NilValue() || coef == R_NilValue() || XLENGTH(data) == 0 {
            return R_NilValue();
        }
        let q = elt_real_or(coef, 0.0);
        let r = if XLENGTH(coef) > 1 {
            *REAL(coef).add(1)
        } else {
            elt_real_or(coef, 0.0)
        };
        let a0 = *REAL(data);
        let p0 = if r > 0.0 { 1e4 * r } else { 1e4 };
        let z = Rf_ScalarReal(1.0);
        let _z = protect(z);
        let a = Rf_ScalarReal(a0);
        let _a = protect(a);
        let p = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 1, 1);
        let _p = protect(p);
        *REAL(p) = 0.0;
        let t = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 1, 1);
        let _t = protect(t);
        *REAL(t) = 1.0;
        let v = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 1, 1);
        let _v = protect(v);
        *REAL(v) = q;
        let h = Rf_ScalarReal(r);
        let _h = protect(h);
        let pn = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 1, 1);
        let _pn = protect(pn);
        *REAL(pn) = p0;
        let model = Rf_allocVector3(SEXPTYPE::VECSXP, 7);
        let _m = protect(model);
        SET_VECTOR_ELT(model, 0, z);
        SET_VECTOR_ELT(model, 1, a);
        SET_VECTOR_ELT(model, 2, p);
        SET_VECTOR_ELT(model, 3, t);
        SET_VECTOR_ELT(model, 4, v);
        SET_VECTOR_ELT(model, 5, h);
        SET_VECTOR_ELT(model, 6, pn);
        crate::mainutils::essentials::set_string_names(
            model,
            &[
                "Z".to_string(),
                "a".to_string(),
                "P".to_string(),
                "T".to_string(),
                "V".to_string(),
                "h".to_string(),
                "Pn".to_string(),
            ],
        );
        let ks_args = Rf_cons(data, Rf_cons(model, R_NilValue()));
        let _ka = protect(ks_args);
        let ks = do_kalman_smooth(call, op, ks_args, rho);
        let _ks = protect(ks);
        named_list_elt(ks, "smooth")
    }
}











/// GNU `spec.taper(x, p=0.1)` — cosine taper on each end.
pub unsafe fn do_spec_taper(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mut p = 0.1;
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let pv = CAR(rest);
            if !pv.is_null() && pv != R_NilValue() {
                p = if TYPEOF(pv) == SEXPTYPE::REALSXP {
                    *REAL(pv)
                } else if TYPEOF(pv) == SEXPTYPE::INTSXP {
                    *INTEGER(pv) as f64
                } else {
                    0.1
                };
            }
        }
        if p < 0.0 {
            p = 0.0;
        }
        if p > 0.5 {
            p = 0.5;
        }
        let n = XLENGTH(x) as usize;
        let m = ((n as f64) * p).floor() as usize;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _r = protect(result);
        for i in 0..n {
            let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else {
                *INTEGER(x).add(i) as f64
            };
            *REAL(result).add(i) = v;
        }
        if m == 0 || n == 0 {
            return result;
        }
        for k in 0..m {
            let odd = (2 * k + 1) as f64;
            let w = 0.5 * (1.0 - (std::f64::consts::PI * odd / (2.0 * m as f64)).cos());
            *REAL(result).add(k) *= w;
            *REAL(result).add(n - 1 - k) *= w;
        }
        result
    }
}


/// GNU `spec.ar(x)` AR(1) spectral density.
pub unsafe fn do_spec_ar(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x0 = CAR(args);
        let n = XLENGTH(x0) as usize;
        let mut x = vec![0.0f64; n];
        let mut mean = 0.0;
        for i in 0..n {
            x[i] = if TYPEOF(x0) == SEXPTYPE::REALSXP {
                *REAL(x0).add(i)
            } else {
                *INTEGER(x0).add(i) as f64
            };
            mean += x[i];
        }
        mean /= n as f64;
        let a = do_ar(_call, _op, args, rho);
        let _a = protect(a);
        let phi = *REAL(VECTOR_ELT(a, 0));
        let mut r0 = 0.0;
        for i in 0..n {
            let d = x[i] - mean;
            r0 += d * d;
        }
        r0 /= n as f64;
        let vp = r0 * (1.0 - phi * phi) * (n as f64) / (n as f64 - 2.0);
        let nfreq = 500i64;
        let spec = Rf_allocVector3(SEXPTYPE::REALSXP, nfreq);
        let _s = protect(spec);
        for k in 0..nfreq as usize {
            let freq = 0.5 * k as f64 / (nfreq as f64 - 1.0);
            let den = 1.0 + phi * phi - 2.0 * phi * (2.0 * std::f64::consts::PI * freq).cos();
            *REAL(spec).add(k) = vp / den;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, spec);
        crate::mainutils::essentials::set_string_names(result, &["spec".to_string()]);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"spec".as_ptr()),
        );
        result
    }
}


/// GNU additive `decompose(ts)` via centered moving average.
pub unsafe fn do_decompose(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x0 = CAR(args);
        let n = XLENGTH(x0) as usize;
        let tsp = crate::sexp::attrib_core::getAttrib(
            x0,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
        );
        let freq = if !tsp.is_null()
            && TYPEOF(tsp) == SEXPTYPE::REALSXP
            && XLENGTH(tsp) >= 3
        {
            *REAL(tsp).add(2) as usize
        } else {
            1
        };
        if freq < 2 || n < 2 * freq {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "time series has no or less than 2 periods",
            );
        }
        let mut x = vec![0.0f64; n];
        for i in 0..n {
            x[i] = if TYPEOF(x0) == SEXPTYPE::REALSXP {
                *REAL(x0).add(i)
            } else {
                *INTEGER(x0).add(i) as f64
            };
        }
        let half = freq / 2;
        let even = freq % 2 == 0;
        let trend_s = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _t = protect(trend_s);
        for i in 0..n {
            if i < half || i + half >= n || (even && i + half >= n) {
                *REAL(trend_s).add(i) = NA_REAL;
                continue;
            }
            if even {
                if i < half || i + half >= n {
                    *REAL(trend_s).add(i) = NA_REAL;
                    continue;
                }
                let mut s = 0.5 * x[i - half] + 0.5 * x[i + half];
                for k in (i - half + 1)..(i + half) {
                    s += x[k];
                }
                *REAL(trend_s).add(i) = s / freq as f64;
            } else {
                let mut s = 0.0;
                for k in (i - half)..=(i + half) {
                    s += x[k];
                }
                *REAL(trend_s).add(i) = s / freq as f64;
            }
        }
        let mut fig = vec![0.0f64; freq];
        let mut cnt = vec![0.0f64; freq];
        for i in 0..n {
            let tr = *REAL(trend_s).add(i);
            if tr.is_nan() {
                continue;
            }
            let k = i % freq;
            fig[k] += x[i] - tr;
            cnt[k] += 1.0;
        }
        for k in 0..freq {
            if cnt[k] > 0.0 {
                fig[k] /= cnt[k];
            }
        }
        let mean = fig.iter().sum::<f64>() / freq as f64;
        for k in 0..freq {
            fig[k] -= mean;
        }
        let seasonal = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _s = protect(seasonal);
        let random = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _r = protect(random);
        for i in 0..n {
            *REAL(seasonal).add(i) = fig[i % freq];
            let tr = *REAL(trend_s).add(i);
            if tr.is_nan() {
                *REAL(random).add(i) = NA_REAL;
            } else {
                *REAL(random).add(i) = x[i] - fig[i % freq] - tr;
            }
        }
        let figure = Rf_allocVector3(SEXPTYPE::REALSXP, freq as i64);
        let _fg = protect(figure);
        for k in 0..freq {
            *REAL(figure).add(k) = fig[k];
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 6);
        let _res = protect(result);
        SET_VECTOR_ELT(result, 0, seasonal);
        SET_VECTOR_ELT(result, 1, trend_s);
        SET_VECTOR_ELT(result, 2, random);
        SET_VECTOR_ELT(result, 3, figure);
        SET_VECTOR_ELT(result, 4, x0);
        SET_VECTOR_ELT(result, 5, Rf_mkString(c"additive".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "seasonal".to_string(),
                "trend".to_string(),
                "random".to_string(),
                "figure".to_string(),
                "x".to_string(),
                "type".to_string(),
            ],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"decomposed.ts".as_ptr()),
        );
        result
    }
}

/// GNU `ARMAacf(ar, lag.max)` for AR(1).
pub unsafe fn do_ARMAacf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut phi = 0.0;
        let mut phi2 = 0.0;
        let mut ar_len = 0i64;
        let mut theta = 0.0;
        let mut has_ma = false;
        let mut lag_max = 1i64;
        let mut p = args;
        while !p.is_null() && p != R_NilValue() {
            let tag = TAG(p);
            let name = if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let v = CAR(p);
            if name == "ar" || (name.is_empty() && p == args) {
                ar_len = XLENGTH(v);
                if TYPEOF(v) == SEXPTYPE::REALSXP {
                    phi = *REAL(v);
                    if ar_len >= 2 {
                        phi2 = *REAL(v).add(1);
                    }
                } else {
                    phi = *INTEGER(v) as f64;
                    if ar_len >= 2 {
                        phi2 = *INTEGER(v).add(1) as f64;
                    }
                }
            } else if name == "ma" {
                has_ma = true;
                theta = if TYPEOF(v) == SEXPTYPE::REALSXP {
                    *REAL(v)
                } else {
                    *INTEGER(v) as f64
                };
            } else if name == "lag.max" {
                lag_max = if TYPEOF(v) == SEXPTYPE::INTSXP {
                    *INTEGER(v) as i64
                } else {
                    *REAL(v) as i64
                };
            }
            p = CDR(p);
        }
        let n = lag_max + 1;
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _a = protect(ans);
        if has_ma && phi == 0.0 && ar_len <= 1 {
            *REAL(ans) = 1.0;
            if n > 1 {
                *REAL(ans).add(1) = theta / (1.0 + theta * theta);
            }
            for i in 2..n as usize {
                *REAL(ans).add(i) = 0.0;
            }
        } else if ar_len >= 2 {
            *REAL(ans) = 1.0;
            if n > 1 {
                *REAL(ans).add(1) = phi / (1.0 - phi2);
            }
            if n > 2 {
                *REAL(ans).add(2) = phi2 + phi * *REAL(ans).add(1);
            }
            for i in 3..n as usize {
                *REAL(ans).add(i) =
                    phi * *REAL(ans).add(i - 1) + phi2 * *REAL(ans).add(i - 2);
            }
        } else {
            let mut acc = 1.0;
            for i in 0..n as usize {
                *REAL(ans).add(i) = acc;
                acc *= phi;
            }
        }
        let names: Vec<String> = (0..=lag_max).map(|i| i.to_string()).collect();
        crate::mainutils::essentials::set_string_names(ans, &names);
        ans
    }
}

/// GNU `ARMAtoMA(ar, lag.max)` for AR(1).
pub unsafe fn do_ARMAtoMA(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut phi = 0.0;
        let mut lag_max = 1i64;
        let mut p = args;
        while !p.is_null() && p != R_NilValue() {
            let tag = TAG(p);
            let name = if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let v = CAR(p);
            if name == "ar" || name.is_empty() && p == args {
                phi = if TYPEOF(v) == SEXPTYPE::REALSXP {
                    *REAL(v)
                } else {
                    *INTEGER(v) as f64
                };
            } else if name == "lag.max" {
                lag_max = if TYPEOF(v) == SEXPTYPE::INTSXP {
                    *INTEGER(v) as i64
                } else {
                    *REAL(v) as i64
                };
            }
            p = CDR(p);
        }
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, lag_max);
        let _a = protect(ans);
        let mut acc = phi;
        for i in 0..lag_max as usize {
            *REAL(ans).add(i) = acc;
            acc *= phi;
        }
        ans
    }
}

/// GNU `acf2AR(acf)` successive Yule-Walker AR fits.
pub unsafe fn do_acf2AR(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let acf = CAR(args);
        let acf = if TYPEOF(acf) == SEXPTYPE::REALSXP {
            acf
        } else {
            coerceVector(acf, SEXPTYPE::REALSXP.as_c_int())
        };
        let _acf = protect(acf);
        let n = XLENGTH(acf) as usize;
        if n < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "acf must have at least two lags",
            );
        }
        let p = n - 1;
        let mut rho = vec![0.0f64; n];
        for i in 0..n {
            rho[i] = *REAL(acf).add(i);
        }
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), p as i32, p as i32);
        let _m = protect(mat);
        for i in 0..(p * p) {
            *REAL(mat).add(i) = 0.0;
        }
        let mut phi_prev = vec![0.0f64; p];
        for k in 1..=p {
            let mut num = rho[k];
            let mut den = 1.0;
            for j in 1..k {
                num -= phi_prev[j - 1] * rho[k - j];
                den -= phi_prev[j - 1] * rho[j];
            }
            let phikk = if den.abs() < 1e-15 { 0.0 } else { num / den };
            let mut phi = vec![0.0f64; k];
            for j in 1..k {
                phi[j - 1] = phi_prev[j - 1] - phikk * phi_prev[k - j - 1];
            }
            phi[k - 1] = phikk;
            for j in 0..k {
                *REAL(mat).add((k - 1) + j * p) = phi[j];
            }
            for j in 0..k {
                phi_prev[j] = phi[j];
            }
        }
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, p as i64);
        let _rn = protect(rn);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, p as i64);
        let _cn = protect(cn);
        for i in 0..p {
            let rlab = format!("ar({})\0", i + 1);
            let clab = format!("{}\0", i + 1);
            SET_STRING_ELT(rn, i as i64, Rf_mkChar(rlab.as_ptr() as *const _));
            SET_STRING_ELT(cn, i as i64, Rf_mkChar(clab.as_ptr() as *const _));
        }
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(
            mat,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dn,
        );
        mat
    }
}

/// GNU `kernel("daniell", m)`.
pub unsafe fn do_kernel(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m_s = CAR(CDR(args));
        let m = if TYPEOF(m_s) == SEXPTYPE::INTSXP {
            *INTEGER(m_s)
        } else {
            *REAL(m_s) as c_int
        };
        if m < 0 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'m' must be non-negative",
            );
        }
        let w = 1.0 / (2.0 * m as f64 + 1.0);
        let coef = Rf_allocVector3(SEXPTYPE::REALSXP, (m + 1) as i64);
        let _c = protect(coef);
        for i in 0..=m as usize {
            *REAL(coef).add(i) = w;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, coef);
        SET_VECTOR_ELT(result, 1, Rf_ScalarInteger(m));
        crate::mainutils::essentials::set_string_names(
            result,
            &["coef".to_string(), "m".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"tskernel".as_ptr()),
        );
        result
    }
}

/// GNU `df.kernel(k)` — equivalent degrees of freedom.
pub unsafe fn do_df_kernel(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let k = CAR(args);
        let coef = VECTOR_ELT(k, 0);
        let m_s = VECTOR_ELT(k, 1);
        let m = if TYPEOF(m_s) == SEXPTYPE::INTSXP {
            *INTEGER(m_s) as usize
        } else {
            *REAL(m_s) as usize
        };
        let mut ss = (*REAL(coef)).powi(2);
        for j in 1..=m {
            ss += 2.0 * (*REAL(coef).add(j)).powi(2);
        }
        Rf_ScalarReal(if ss > 0.0 { 2.0 / ss } else { f64::NAN })
    }
}

/// GNU `bandwidth.kernel(k)` — equivalent bandwidth.
pub unsafe fn do_bandwidth_kernel(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let k = CAR(args);
        let coef = VECTOR_ELT(k, 0);
        let m_s = VECTOR_ELT(k, 1);
        let m = if TYPEOF(m_s) == SEXPTYPE::INTSXP {
            *INTEGER(m_s) as usize
        } else {
            *REAL(m_s) as usize
        };
        let mut s = *REAL(coef) / 12.0;
        for i in 1..=m {
            let w = *REAL(coef).add(i);
            s += 2.0 * (1.0 / 12.0 + (i as f64) * (i as f64)) * w;
        }
        Rf_ScalarReal(s.max(0.0).sqrt())
    }
}



/// GNU `kernapply(x, k)` two-sided Daniell.
pub unsafe fn do_kernapply(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let k = CAR(CDR(args));
        let xd = if TYPEOF(x) == SEXPTYPE::REALSXP {
            x
        } else {
            coerceVector(x, SEXPTYPE::REALSXP.as_c_int())
        };
        let _xd = protect(xd);
        let n = XLENGTH(xd) as usize;
        let m_s = VECTOR_ELT(k, 1);
        let m = if TYPEOF(m_s) == SEXPTYPE::INTSXP {
            *INTEGER(m_s) as usize
        } else {
            *REAL(m_s) as usize
        };
        let coef = VECTOR_ELT(k, 0);
        let out_n = n.saturating_sub(2 * m);
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, out_n as i64);
        let _a = protect(ans);
        for i in 0..out_n {
            let t = i + m;
            let mut s = *REAL(coef) * *REAL(xd).add(t);
            for j in 1..=m {
                let w = *REAL(coef).add(j);
                s += w * *REAL(xd).add(t - j) + w * *REAL(xd).add(t + j);
            }
            *REAL(ans).add(i) = s;
        }
        ans
    }
}

/// GNU `is.tskernel(x)`.
pub unsafe fn do_is_tskernel(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        let mut ok = false;
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(class) {
                let s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(class, i)));
                if s.to_bytes() == b"tskernel" {
                    ok = true;
                    break;
                }
            }
        }
        Rf_ScalarLogical(if ok { 1 } else { 0 })
    }
}

unsafe fn ts_start_freq(x: SEXP) -> (f64, f64) {
    unsafe {
        let tsp = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
        );
        if !tsp.is_null() && TYPEOF(tsp) == SEXPTYPE::REALSXP && XLENGTH(tsp) >= 3 {
            (*REAL(tsp), *REAL(tsp).add(2))
        } else {
            (1.0, 1.0)
        }
    }
}

/// GNU `cycle(ts)`.
pub unsafe fn do_cycle(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x);
        let (start, freq) = ts_start_freq(x);
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _a = protect(ans);
        let start_cycle = ((start - 1.0) * freq).round();
        for i in 0..n as usize {
            let cyc = (start_cycle + i as f64).rem_euclid(freq) + 1.0;
            *REAL(ans).add(i) = cyc;
        }
        ans
    }
}

/// GNU `time(ts)`.
pub unsafe fn do_time(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x);
        let (start, freq) = ts_start_freq(x);
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _a = protect(ans);
        for i in 0..n as usize {
            *REAL(ans).add(i) = start + i as f64 / freq;
        }
        ans
    }
}

/// GNU `as.ts(x)`.
pub unsafe fn do_as_ts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'ts' object must have one or more observations",
            );
        }
        let n = XLENGTH(x);
        if n == 0 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'ts' object must have one or more observations",
            );
        }
        let existing = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
        );
        if !existing.is_null() && existing != R_NilValue() {
            return x;
        }
        let tsp = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        let _t = protect(tsp);
        *REAL(tsp) = 1.0;
        *REAL(tsp).add(1) = n as f64;
        *REAL(tsp).add(2) = 1.0;
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::symbol::Rf_install(c"tsp".as_ptr()), tsp);
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"ts".as_ptr()),
        );
        x
    }
}

/// GNU `plot.ts(x)` — `as.ts(x)` then plot.
pub unsafe fn do_plot_ts(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let as_args = Rf_cons(x, R_NilValue());
        let _aa = protect(as_args);
        let _ = do_as_ts(call, op, as_args, rho);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// GNU `ts.plot(...)` — `ts.union(...)` then `plot.ts`.
pub unsafe fn do_ts_plot(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let sers = crate::mainutils::essentials::do_ts_union(call, op, args, rho);
        let _s = protect(sers);
        let plot_args = Rf_cons(sers, R_NilValue());
        let _pa = protect(plot_args);
        do_plot_ts(call, op, plot_args, rho)
    }
}



/// GNU `hasTsp(x)` — ensure a `tsp` attribute, do not set class.
pub unsafe fn do_has_tsp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let existing = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
        );
        if !existing.is_null() && existing != R_NilValue() {
            return x;
        }
        let n = XLENGTH(x);
        let tsp = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        let _t = protect(tsp);
        *REAL(tsp) = 1.0;
        *REAL(tsp).add(1) = n as f64;
        *REAL(tsp).add(2) = 1.0;
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::symbol::Rf_install(c"tsp".as_ptr()), tsp);
        x
    }
}


/// GNU `window(ts, start, end)`.
pub unsafe fn do_window(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let start = CAR(CDR(args));
        let end = CAR(CDR(CDR(args)));
        let (t0, freq) = ts_start_freq(x);
        let s = if TYPEOF(start) == SEXPTYPE::REALSXP {
            *REAL(start)
        } else {
            *INTEGER(start) as f64
        };
        let e = if TYPEOF(end) == SEXPTYPE::REALSXP {
            *REAL(end)
        } else {
            *INTEGER(end) as f64
        };
        let i0 = ((s - t0) * freq).round() as i64;
        let i1 = ((e - t0) * freq).round() as i64;
        let n = XLENGTH(x);
        let lo = i0.max(0);
        let hi = i1.min(n - 1);
        let out_n = (hi - lo + 1).max(0);
        let ty = TYPEOF(x);
        let ans = Rf_allocVector3(
            if ty == SEXPTYPE::INTSXP {
                SEXPTYPE::INTSXP
            } else {
                SEXPTYPE::REALSXP
            },
            out_n,
        );
        let _a = protect(ans);
        for i in 0..out_n as usize {
            let src = (lo as usize) + i;
            if ty == SEXPTYPE::INTSXP {
                *INTEGER(ans).add(i) = *INTEGER(x).add(src);
            } else {
                *REAL(ans).add(i) = *REAL(x).add(src);
            }
        }
        ans
    }
}

/// GNU `aggregate.ts(x, nfrequency=1, FUN=sum)` — block sums.
pub unsafe fn do_aggregate_ts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let (t0, freq) = ts_start_freq(x);
        let nfreq_s = CAR(CDR(args));
        let nfreq = if nfreq_s.is_null()
            || nfreq_s == R_NilValue()
            || nfreq_s == crate::sexp::globals::R_MissingArg()
        {
            1.0
        } else if TYPEOF(nfreq_s) == SEXPTYPE::REALSXP {
            *REAL(nfreq_s)
        } else {
            *INTEGER(nfreq_s) as f64
        };
        let len = (freq / nfreq).round() as usize;
        if len == 0 {
            return x;
        }
        let n = XLENGTH(x) as usize;
        let nout = n / len;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, nout as i64);
        let _r = protect(result);
        for i in 0..nout {
            let mut s = 0.0;
            for j in 0..len {
                let idx = i * len + j;
                s += if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(idx)
                } else {
                    *INTEGER(x).add(idx) as f64
                };
            }
            *REAL(result).add(i) = s;
        }
        let tsp = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        let _t = protect(tsp);
        *REAL(tsp) = t0;
        *REAL(tsp).add(1) = t0 + (nout as f64 - 1.0) / nfreq;
        *REAL(tsp).add(2) = nfreq;
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
            tsp,
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"ts".as_ptr()),
        );
        result
    }
}


/// GNU `window(x, start, end) <- value` — replace a freq-1 window.
pub unsafe fn do_window_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let start = CAR(CDR(args));
        let end = CAR(CDR(CDR(args)));
        let value = CAR(CDR(CDR(CDR(args))));
        let (t0, freq) = ts_start_freq(x);
        let s = if TYPEOF(start) == SEXPTYPE::REALSXP {
            *REAL(start)
        } else {
            *INTEGER(start) as f64
        };
        let e = if TYPEOF(end) == SEXPTYPE::REALSXP {
            *REAL(end)
        } else {
            *INTEGER(end) as f64
        };
        let val = if TYPEOF(value) == SEXPTYPE::REALSXP {
            *REAL(value)
        } else {
            *INTEGER(value) as f64
        };
        let n = XLENGTH(x) as usize;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _r = protect(result);
        for i in 0..n {
            let t = t0 + i as f64 / freq;
            let src = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else {
                *INTEGER(x).add(i) as f64
            };
            *REAL(result).add(i) = if t + 1e-9 >= s && t <= e + 1e-9 {
                val
            } else {
                src
            };
        }
        let tsp = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
        );
        if !tsp.is_null() && tsp != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
                tsp,
            );
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"ts".as_ptr()),
        );
        result
    }
}


/// GNU `lag(ts, k)` shifts tsp, keeps values.
pub unsafe fn do_lag(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let k_s = CAR(CDR(args));
        let k = if TYPEOF(k_s) == SEXPTYPE::REALSXP {
            *REAL(k_s)
        } else {
            *INTEGER(k_s) as f64
        };
        let (start, freq) = ts_start_freq(x);
        let n = XLENGTH(x) as f64;
        let tsp = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        let _t = protect(tsp);
        *REAL(tsp) = start - k / freq;
        *REAL(tsp).add(1) = start - k / freq + (n - 1.0) / freq;
        *REAL(tsp).add(2) = freq;
        let ans = if TYPEOF(x) == SEXPTYPE::INTSXP {
            let a = Rf_allocVector3(SEXPTYPE::INTSXP, n as i64);
            for i in 0..n as usize {
                *INTEGER(a).add(i) = *INTEGER(x).add(i);
            }
            a
        } else {
            let a = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
            for i in 0..n as usize {
                *REAL(a).add(i) = *REAL(x).add(i);
            }
            a
        };
        let _a = protect(ans);
        crate::sexp::attrib_core::setAttrib(
            ans,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
            tsp,
        );
        crate::sexp::attrib_core::setAttrib(
            ans,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"ts".as_ptr()),
        );
        ans
    }
}

/// GNU `frequency(ts)`.
pub unsafe fn do_frequency(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let (_s, freq) = ts_start_freq(CAR(args));
        Rf_ScalarReal(freq)
    }
}

/// GNU `deltat(ts)`.
pub unsafe fn do_deltat(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let (_s, freq) = ts_start_freq(CAR(args));
        Rf_ScalarReal(1.0 / freq)
    }
}

unsafe fn time_to_ycyc(t: f64, freq: f64) -> (f64, f64) {
    let mut year = t.floor();
    let mut cyc = ((t - year) * freq).round() + 1.0;
    if cyc > freq {
        year += 1.0;
        cyc = 1.0;
    }
    if cyc < 1.0 {
        year -= 1.0;
        cyc = freq;
    }
    (year, cyc)
}

/// GNU `start(ts)` as c(year, cycle).
pub unsafe fn do_start(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let (start, freq) = ts_start_freq(x);
        let (y, c) = time_to_ycyc(start, freq);
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _a = protect(ans);
        *REAL(ans) = y;
        *REAL(ans).add(1) = c;
        ans
    }
}

/// GNU `end(ts)` as c(year, cycle).
pub unsafe fn do_end(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x);
        let (start, freq) = ts_start_freq(x);
        let t = start + (n as f64 - 1.0) / freq;
        let (y, c) = time_to_ycyc(t, freq);
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _a = protect(ans);
        *REAL(ans) = y;
        *REAL(ans).add(1) = c;
        ans
    }
}

/// GNU `is.ts(x)`.
pub unsafe fn do_is_ts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        let mut ok = false;
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(class) {
                let s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(class, i)));
                if s.to_bytes() == b"ts" {
                    ok = true;
                    break;
                }
            }
        }
        Rf_ScalarLogical(if ok { 1 } else { 0 })
    }
}

/// GNU `is.mts(x)` — ts matrix with class `mts`.
pub unsafe fn do_is_mts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        let mut has_ts = false;
        let mut has_mts = false;
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(class) {
                let s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(class, i)));
                if s.to_bytes() == b"ts" {
                    has_ts = true;
                }
                if s.to_bytes() == b"mts" {
                    has_mts = true;
                }
            }
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let is_mat = !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2;
        Rf_ScalarLogical(if has_ts && has_mts && is_mat { 1 } else { 0 })
    }
}


/// GNU `as.stepfun(x)` — identity when `is.stepfun(x)`.
pub unsafe fn do_as_stepfun(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { CAR(args) }
}

/// GNU `is.leaf(object)` — TRUE when `attr(*, "leaf")` is TRUE.
pub unsafe fn do_is_leaf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let leaf = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"leaf".as_ptr()),
        );
        let ok = !leaf.is_null()
            && leaf != R_NilValue()
            && TYPEOF(leaf) == SEXPTYPE::LGLSXP
            && XLENGTH(leaf) > 0
            && *LOGICAL(leaf) == 1;
        Rf_ScalarLogical(if ok { 1 } else { 0 })
    }
}

/// GNU `is.stepfun(x)` — function that inherits class `stepfun`.
pub unsafe fn do_is_stepfun(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let is_fun = TYPEOF(x) == SEXPTYPE::CLOSXP
            || TYPEOF(x) == SEXPTYPE::BUILTINSXP
            || TYPEOF(x) == SEXPTYPE::SPECIALSXP;
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        let mut has = false;
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(class) {
                let s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(class, i)));
                if s.to_bytes() == b"stepfun" {
                    has = true;
                    break;
                }
            }
        }
        Rf_ScalarLogical(if is_fun && has { 1 } else { 0 })
    }
}




/// GNU `na.contiguous(x)` longest non-NA run.
pub unsafe fn do_na_contiguous(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x) as usize;
        let is_int = TYPEOF(x) == SEXPTYPE::INTSXP;
        let na_at = |i: usize| -> bool {
            if is_int {
                *INTEGER(x).add(i) == NA_INTEGER
            } else {
                let v = *REAL(x).add(i);
                v.is_nan()
            }
        };
        let mut best_lo = 0usize;
        let mut best_len = 0usize;
        let mut i = 0;
        while i < n {
            if na_at(i) {
                i += 1;
                continue;
            }
            let lo = i;
            while i < n && !na_at(i) {
                i += 1;
            }
            let len = i - lo;
            if len > best_len {
                best_lo = lo;
                best_len = len;
            }
        }
        let ans = if is_int {
            Rf_allocVector3(SEXPTYPE::INTSXP, best_len as i64)
        } else {
            Rf_allocVector3(SEXPTYPE::REALSXP, best_len as i64)
        };
        let _a = protect(ans);
        for j in 0..best_len {
            if is_int {
                *INTEGER(ans).add(j) = *INTEGER(x).add(best_lo + j);
            } else {
                *REAL(ans).add(j) = *REAL(x).add(best_lo + j);
            }
        }
        let omit_n = n - best_len;
        let omit = Rf_allocVector3(SEXPTYPE::INTSXP, omit_n as i64);
        let _o = protect(omit);
        let mut k = 0usize;
        for j in 0..n {
            if j < best_lo || j >= best_lo + best_len {
                *INTEGER(omit).add(k) = (j + 1) as c_int;
                k += 1;
            }
        }
        crate::sexp::attrib_core::setAttrib(
            omit,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"omit".as_ptr()),
        );
        crate::sexp::attrib_core::setAttrib(
            ans,
            crate::sexp::symbol::Rf_install(c"na.action".as_ptr()),
            omit,
        );
        ans
    }
}

/// GNU `na.pass(x)`.
pub unsafe fn do_na_pass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { CAR(args) }
}

/// GNU `napredict(omit, x)` with NULL omit returns x.
pub unsafe fn do_napredict(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { CAR(CDR(args)) }
}

/// GNU `na.fail(x)`.
pub unsafe fn do_na_fail(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x) as usize;
        let mut missing = false;
        if TYPEOF(x) == SEXPTYPE::REALSXP {
            for i in 0..n {
                if (*REAL(x).add(i)).is_nan() {
                    missing = true;
                    break;
                }
            }
        } else if TYPEOF(x) == SEXPTYPE::INTSXP {
            for i in 0..n {
                if *INTEGER(x).add(i) == NA_INTEGER {
                    missing = true;
                    break;
                }
            }
        }
        if missing {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "missing values in object",
            );
        }
        x
    }
}

/// GNU `labels.default` — names, else `as.character(seq_along(x))`.
pub unsafe fn do_labels(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        if !class.is_null() && class != R_NilValue() && TYPEOF(class) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(class) {
                let raw = CHAR(STRING_ELT(class, i));
                if !raw.is_null() && std::ffi::CStr::from_ptr(raw).to_bytes() == b"dist" {
                    return crate::sexp::attrib_core::getAttrib(
                        x,
                        crate::sexp::symbol::Rf_install(c"Labels".as_ptr()),
                    );
                }
            }
        }
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        if !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && XLENGTH(names) == XLENGTH(x)
        {
            return names;
        }
        let n = XLENGTH(x);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for i in 0..n {
            let s = std::ffi::CString::new((i + 1).to_string()).unwrap_or_default();
            SET_STRING_ELT(out, i, Rf_mkChar(s.as_ptr()));
        }
        out
    }
}

/// GNU `na.action` — `$na.action` or `attr(, "na.action")`.
pub unsafe fn do_na_action(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) == SEXPTYPE::VECSXP {
            let v = named_list_elt(x, "na.action");
            if !v.is_null() && v != R_NilValue() {
                return v;
            }
        }
        crate::sexp::attrib_core::getAttrib(x, crate::sexp::symbol::Rf_install(c"na.action".as_ptr()))
    }
}

/// GNU `model.weights(x)` — `x[["(weights)"]]`.
pub unsafe fn do_model_weights(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { named_list_elt(CAR(args), "(weights)") }
}

/// GNU `model.matrix(~x)` — intercept plus one numeric column.
pub unsafe fn do_model_matrix(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let form = CAR(args);
        if form.is_null() || form == R_NilValue() || TYPEOF(form) != SEXPTYPE::LANGSXP {
            return R_NilValue();
        }
        let rhs_cell = CDR(CDR(form));
        let rhs = if rhs_cell.is_null() || rhs_cell == R_NilValue() {
            CADR(form)
        } else {
            CAR(rhs_cell)
        };
        if rhs.is_null() || rhs == R_NilValue() {
            return R_NilValue();
        }
        let xname = if TYPEOF(rhs) == SEXPTYPE::SYMSXP {
            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(rhs)))
                .to_string_lossy()
                .into_owned()
        } else {
            "x".to_string()
        };
        let x = crate::eval::eval::Rf_eval(rhs, rho);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        if n <= 0 {
            return R_NilValue();
        }
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, 2);
        let _m = protect(mat);
        for i in 0..n as usize {
            *REAL(mat).add(i) = 1.0;
            let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                let iv = *INTEGER(x).add(i);
                if iv == NA_INTEGER {
                    f64::NAN
                } else {
                    iv as f64
                }
            } else {
                f64::NAN
            };
            *REAL(mat).add(i + n as usize) = v;
        }
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _rn = protect(rn);
        for i in 0..n {
            let lab = format!("{}\0", i + 1);
            SET_STRING_ELT(rn, i, Rf_mkChar(lab.as_ptr() as *const _));
        }
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _cn = protect(cn);
        SET_STRING_ELT(cn, 0, Rf_mkChar(c"(Intercept)".as_ptr()));
        let xn = std::ffi::CString::new(xname).unwrap_or_default();
        SET_STRING_ELT(cn, 1, Rf_mkChar(xn.as_ptr()));
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(
            mat,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dn,
        );
        let assign = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        let _as = protect(assign);
        *INTEGER(assign) = 0;
        *INTEGER(assign).add(1) = 1;
        crate::sexp::attrib_core::setAttrib(
            mat,
            crate::sexp::symbol::Rf_install(c"assign".as_ptr()),
            assign,
        );
        mat
    }
}

/// GNU `model.matrix.default(y ~ x, data)` — intercept plus `x`.
pub unsafe fn do_model_matrix_default(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        let data = CAR(CDR(args));
        if data.is_null() || data == R_NilValue() || TYPEOF(data) != SEXPTYPE::VECSXP {
            return do_model_matrix(call, op, args, rho);
        }
        let form = CAR(args);
        let rhs_cell = if TYPEOF(form) == SEXPTYPE::LANGSXP {
            CDR(CDR(form))
        } else {
            std::ptr::null_mut()
        };
        let rhs = if rhs_cell.is_null() || rhs_cell == R_NilValue() {
            if TYPEOF(form) == SEXPTYPE::LANGSXP {
                CADR(form)
            } else {
                std::ptr::null_mut()
            }
        } else {
            CAR(rhs_cell)
        };
        let xname = if !rhs.is_null() && TYPEOF(rhs) == SEXPTYPE::SYMSXP {
            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(rhs)))
                .to_string_lossy()
                .into_owned()
        } else {
            "x".to_string()
        };
        let names = crate::sexp::attrib_core::getAttrib(
            data,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let mut col = R_NilValue();
        if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(names) {
                let nm = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i)))
                    .to_string_lossy();
                if nm == xname {
                    col = VECTOR_ELT(data, i);
                    break;
                }
            }
        }
        if col.is_null() || col == R_NilValue() {
            return do_model_matrix(call, op, args, rho);
        }
        let n = XLENGTH(col);
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, 2);
        let _m = protect(mat);
        for i in 0..n as usize {
            *REAL(mat).add(i) = 1.0;
            *REAL(mat).add(i + n as usize) = if TYPEOF(col) == SEXPTYPE::REALSXP {
                *REAL(col).add(i)
            } else {
                *INTEGER(col).add(i) as f64
            };
        }
        mat
    }
}


/// GNU `model.matrix.lm(object)` — intercept plus `1:n` when `$x` is missing.
pub unsafe fn do_model_matrix_lm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let x = named_list_elt(obj, "x");
        if !x.is_null() && x != R_NilValue() {
            return x;
        }
        let resid = named_list_elt(obj, "residuals");
        if resid.is_null() || resid == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid) as usize;
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, 2);
        let _m = protect(mat);
        for i in 0..n {
            *REAL(mat).add(i) = 1.0;
            *REAL(mat).add(i + n) = (i + 1) as f64;
        }
        mat
    }
}


/// GNU `reformulate(termlabels, response=NULL)` — build a formula.
pub unsafe fn do_reformulate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let labels = CAR(args);
        if labels.is_null() || labels == R_NilValue() || TYPEOF(labels) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        let n = XLENGTH(labels);
        if n <= 0 {
            return R_NilValue();
        }
        let plus = crate::sexp::symbol::Rf_install(c"+".as_ptr());
        let mut rhs = R_NilValue();
        for i in 0..n {
            let raw = CHAR(STRING_ELT(labels, i));
            if raw.is_null() {
                continue;
            }
            let lab = std::ffi::CStr::from_ptr(raw).to_string_lossy();
            let term = if lab.as_ref() == "1" {
                Rf_ScalarInteger(1)
            } else {
                let c = std::ffi::CString::new(lab.as_ref()).unwrap_or_default();
                crate::sexp::symbol::Rf_install(c.as_ptr())
            };
            rhs = if rhs == R_NilValue() {
                term
            } else {
                let node = Rf_lang3(plus, rhs, term);
                let _n = protect(node);
                node
            };
        }
        if rhs == R_NilValue() {
            return R_NilValue();
        }
        let tilde = crate::sexp::symbol::Rf_install(c"~".as_ptr());
        let resp = CAR(CDR(args));
        let form = if resp.is_null() || resp == R_NilValue() {
            Rf_lang2(tilde, rhs)
        } else if TYPEOF(resp) == SEXPTYPE::STRSXP && XLENGTH(resp) > 0 {
            let raw = CHAR(STRING_ELT(resp, 0));
            let lab = std::ffi::CStr::from_ptr(raw).to_string_lossy();
            let c = std::ffi::CString::new(lab.as_ref()).unwrap_or_default();
            Rf_lang3(tilde, crate::sexp::symbol::Rf_install(c.as_ptr()), rhs)
        } else {
            Rf_lang3(tilde, resp, rhs)
        };
        let _f = protect(form);
        let class = Rf_mkString(c"formula".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            form,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        form
    }
}


fn sexp_is_numeric_zero(x: SEXP) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        if TYPEOF(x) == SEXPTYPE::INTSXP && XLENGTH(x) == 1 {
            return *INTEGER(x) == 0;
        }
        if TYPEOF(x) == SEXPTYPE::REALSXP && XLENGTH(x) == 1 {
            return *REAL(x) == 0.0;
        }
        false
    }
}

fn sexp_is_numeric_one(x: SEXP) -> bool {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::INTSXP && XLENGTH(x) == 1 {
            return *INTEGER(x) == 1;
        }
        if TYPEOF(x) == SEXPTYPE::REALSXP && XLENGTH(x) == 1 {
            return *REAL(x) == 1.0;
        }
        false
    }
}

/// GNU `is.empty.model(x)` — no terms and no intercept.
pub unsafe fn do_is_empty_model(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::LANGSXP {
            return Rf_ScalarLogical(0);
        }
        let rest = CDR(x);
        let third = if rest.is_null() {
            R_NilValue()
        } else {
            CDR(rest)
        };
        let rhs = if third.is_null() || third == R_NilValue() {
            if rest.is_null() {
                R_NilValue()
            } else {
                CAR(rest)
            }
        } else {
            CAR(third)
        };
        let empty = if sexp_is_numeric_zero(rhs) {
            true
        } else if TYPEOF(rhs) == SEXPTYPE::LANGSXP {
            let op = CAR(rhs);
            let name = if !op.is_null() && TYPEOF(op) == SEXPTYPE::SYMSXP {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(op)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            name == "-" && sexp_is_numeric_one(CADR(rhs))
        } else {
            false
        };
        Rf_ScalarLogical(if empty { 1 } else { 0 })
    }
}

/// GNU `alias.lm` for full-rank models — `list(Model = formula)`.
pub unsafe fn do_alias(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let call = named_list_elt(obj, "call");
        let model = if !call.is_null() && call != R_NilValue() && TYPEOF(call) == SEXPTYPE::LANGSXP {
            CADR(call)
        } else {
            R_NilValue()
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, model);
        let names = Rf_mkString(c"Model".as_ptr());
        let _nm = protect(names);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        let class = Rf_mkString(c"listof".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn collect_formula_symbols(expr: SEXP, out: &mut Vec<String>) {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            return;
        }
        if TYPEOF(expr) == SEXPTYPE::SYMSXP {
            let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(expr)))
                .to_string_lossy()
                .into_owned();
            if !matches!(
                name.as_str(),
                "~" | "+" | "-" | "*" | ":" | "/" | "^" | "I" | "("
            ) {
                if !out.iter().any(|s| s == &name) {
                    out.push(name);
                }
            }
            return;
        }
        if TYPEOF(expr) == SEXPTYPE::LANGSXP {
            let mut cell = CDR(expr);
            while !cell.is_null() && cell != R_NilValue() {
                collect_formula_symbols(CAR(cell), out);
                cell = CDR(cell);
            }
        }
    }
}

fn collect_term_labels(expr: SEXP, out: &mut Vec<String>) {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            return;
        }
        if TYPEOF(expr) == SEXPTYPE::SYMSXP {
            let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(expr)))
                .to_string_lossy()
                .into_owned();
            if !matches!(
                name.as_str(),
                "~" | "+" | "-" | "*" | ":" | "/" | "^" | "I" | "("
            ) {
                if !out.iter().any(|s| s == &name) {
                    out.push(name);
                }
            }
            return;
        }
        if TYPEOF(expr) == SEXPTYPE::LANGSXP {
            let op = CAR(expr);
            if !op.is_null() && TYPEOF(op) == SEXPTYPE::SYMSXP {
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(op)))
                    .to_string_lossy()
                    .into_owned();
                if name == ":" {
                    let mut parts = Vec::new();
                    collect_formula_symbols(CADR(expr), &mut parts);
                    collect_formula_symbols(CADDR(expr), &mut parts);
                    if !parts.is_empty() {
                        let joined = parts.join(":");
                        if !out.iter().any(|s| s == &joined) {
                            out.push(joined);
                        }
                    }
                    return;
                }
            }
            let mut cell = CDR(expr);
            while !cell.is_null() && cell != R_NilValue() {
                collect_term_labels(CAR(cell), out);
                cell = CDR(cell);
            }
        }
    }
}

/// GNU `model.frame(formula, data)` — columns named in the formula.
pub unsafe fn do_model_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let form = CAR(args);
        let data = CAR(CDR(args));
        if form.is_null() || form == R_NilValue() {
            return R_NilValue();
        }
        let mut names = Vec::new();
        collect_formula_symbols(form, &mut names);
        if names.is_empty() {
            return R_NilValue();
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, names.len() as i64);
        let _r = protect(result);
        let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as i64);
        let _on = protect(out_names);
        for (i, name) in names.iter().enumerate() {
            let col = if !data.is_null() && data != R_NilValue() {
                named_list_elt(data, name)
            } else {
                R_NilValue()
            };
            SET_VECTOR_ELT(result, i as i64, col);
            let c = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(out_names, i as i64, Rf_mkChar(c.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            out_names,
        );
        let class = Rf_mkString(c"data.frame".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `expand.model.frame(model, extras)` — add extras to the model frame.
pub unsafe fn do_expand_model_frame(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        let model = CAR(args);
        let extras = CAR(CDR(args));
        let mcall = named_list_elt(model, "call");
        if mcall.is_null() || mcall == R_NilValue() || TYPEOF(mcall) != SEXPTYPE::LANGSXP {
            return R_NilValue();
        }
        let form = CADR(mcall);
        if form.is_null() || form == R_NilValue() {
            return R_NilValue();
        }
        let uargs = Rf_cons(extras, R_NilValue());
        let _u1 = protect(uargs);
        let uargs = Rf_cons(form, uargs);
        let _u2 = protect(uargs);
        let updated = do_update_formula(call, op, uargs, rho);
        let _u = protect(updated);
        let mut names = Vec::new();
        collect_formula_symbols(updated, &mut names);
        if names.is_empty() {
            return R_NilValue();
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, names.len() as i64);
        let _r = protect(result);
        let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as i64);
        let _on = protect(out_names);
        for (i, name) in names.iter().enumerate() {
            let c = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            let sym = crate::sexp::symbol::Rf_install(c.as_ptr());
            let col = crate::eval::eval::Rf_eval(sym, rho);
            SET_VECTOR_ELT(result, i as i64, col);
            SET_STRING_ELT(out_names, i as i64, Rf_mkChar(c.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            out_names,
        );
        result
    }
}


/// GNU `model.response(data)` — first column of a model frame.
pub unsafe fn do_model_response(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let data = CAR(args);
        if data.is_null() || data == R_NilValue() || TYPEOF(data) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        if XLENGTH(data) <= 0 {
            return R_NilValue();
        }
        VECTOR_ELT(data, 0)
    }
}










unsafe fn named_list_elt(x: SEXP, name: &str) -> SEXP {
    unsafe {
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        if names.is_null() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        for i in 0..XLENGTH(x) {
            let s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i)));
            if s.to_string_lossy() == name {
                return VECTOR_ELT(x, i);
            }
        }
        R_NilValue()
    }
}

/// GNU `coef(object)` — `$coefficients`, else `$coef`.
pub unsafe fn do_coef(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let v = named_list_elt(x, "coefficients");
        if !v.is_null() && v != R_NilValue() {
            return v;
        }
        named_list_elt(x, "coef")
    }
}

/// GNU default `fitted(object)`.
pub unsafe fn do_fitted(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { named_list_elt(CAR(args), "fitted.values") }
}

/// GNU default `resid`/`residuals`.
pub unsafe fn do_resid(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { named_list_elt(CAR(args), "residuals") }
}

/// GNU default `deviance(object)`.
pub unsafe fn do_deviance(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { named_list_elt(CAR(args), "deviance") }
}

/// GNU default `df.residual(object)`.
pub unsafe fn do_df_residual(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { named_list_elt(CAR(args), "df.residual") }
}

/// GNU `nobs` — `$nobs`/`$n.obs`, else `nobs.lm`: `sum(weights != 0)` or `NROW(residuals)`.
pub unsafe fn do_nobs(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = named_list_elt(x, "nobs");
        if !n.is_null() && n != R_NilValue() {
            return n;
        }
        let n = named_list_elt(x, "n.obs");
        if !n.is_null() && n != R_NilValue() {
            return n;
        }
        let w = named_list_elt(x, "weights");
        if !w.is_null() && w != R_NilValue() {
            let mut nz = 0i32;
            let nw = XLENGTH(w);
            if TYPEOF(w) == SEXPTYPE::REALSXP {
                for i in 0..nw {
                    let v = *REAL(w).add(i as usize);
                    if v != 0.0 && !v.is_nan() {
                        nz += 1;
                    }
                }
            } else if TYPEOF(w) == SEXPTYPE::INTSXP || TYPEOF(w) == SEXPTYPE::LGLSXP {
                for i in 0..nw {
                    let v = *INTEGER(w).add(i as usize);
                    if v != 0 && v != NA_INTEGER {
                        nz += 1;
                    }
                }
            }
            return Rf_ScalarInteger(nz);
        }
        let r = named_list_elt(x, "residuals");
        if !r.is_null() && r != R_NilValue() {
            return Rf_ScalarInteger(XLENGTH(r) as i32);
        }
        R_NilValue()
    }
}

/// GNU default `weights(object)`.
pub unsafe fn do_weights(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { named_list_elt(CAR(args), "weights") }
}

fn mark_formula(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let class = Rf_mkString(c"formula".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        x
    }
}

/// GNU `update.formula(y ~ x, ~ . + z)` — add a term.
pub unsafe fn do_update_formula(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let old = CAR(args);
        let new = CAR(CDR(args));
        if TYPEOF(old) != SEXPTYPE::LANGSXP || TYPEOF(new) != SEXPTYPE::LANGSXP {
            return old;
        }
        let y = CADR(old);
        let x = CADDR(old);
        let rhs = CADR(new);
        let plus = crate::sexp::symbol::Rf_install(c"+".as_ptr());
        let tilde = crate::sexp::symbol::Rf_install(c"~".as_ptr());
        if !rhs.is_null() && TYPEOF(rhs) == SEXPTYPE::SYMSXP {
            let new_rhs = crate::sexp::constructors::Rf_lang3(plus, x, rhs);
            return mark_formula(crate::sexp::constructors::Rf_lang3(tilde, y, new_rhs));
        }
        if rhs.is_null() || TYPEOF(rhs) != SEXPTYPE::LANGSXP {
            return old;
        }
        let op = CAR(rhs);
        let opname = if TYPEOF(op) == SEXPTYPE::SYMSXP {
            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(op)))
                .to_string_lossy()
                .into_owned()
        } else {
            String::new()
        };
        if opname != "+" {
            return old;
        }
        let left = CADR(rhs);
        let z = CADDR(rhs);
        let left_name = if TYPEOF(left) == SEXPTYPE::SYMSXP {
            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(left)))
                .to_string_lossy()
                .into_owned()
        } else {
            String::new()
        };
        if left_name != "." {
            return old;
        }
        let new_rhs = crate::sexp::constructors::Rf_lang3(plus, x, z);
        mark_formula(crate::sexp::constructors::Rf_lang3(tilde, y, new_rhs))
    }
}


fn parse_formula_text(s: SEXP) -> SEXP {
    unsafe {
        let mut status: std::os::raw::c_int = 0;
        let parsed = crate::mainutils::gram_main::R_ParseVector(s, -1, &mut status, R_NilValue());
        if status != 1 || parsed.is_null() || parsed == R_NilValue() || XLENGTH(parsed) < 1 {
            return R_NilValue();
        }
        mark_formula(VECTOR_ELT(parsed, 0))
    }
}

fn inherits_formula(x: SEXP) -> bool {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return false;
        }
        for i in 0..XLENGTH(class) {
            let raw = CHAR(STRING_ELT(class, i));
            if !raw.is_null() && std::ffi::CStr::from_ptr(raw).to_bytes() == b"formula" {
                return true;
            }
        }
        false
    }
}

/// GNU `formula(object)` — character, language, `$formula`, or `$call`.
pub unsafe fn do_formula(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if inherits_formula(x) {
            return x;
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP && XLENGTH(x) > 0 {
            return parse_formula_text(x);
        }
        if TYPEOF(x) == SEXPTYPE::LANGSXP {
            return mark_formula(x);
        }
        let f = named_list_elt(x, "formula");
        if !f.is_null() && f != R_NilValue() {
            return f;
        }
        let call = named_list_elt(x, "call");
        if !call.is_null() && call != R_NilValue() && TYPEOF(call) == SEXPTYPE::LANGSXP {
            return CADR(call);
        }
        R_NilValue()
    }
}

/// GNU `DF2formula(x)` — first name ~ second name.
pub unsafe fn do_df2formula(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        if names.is_null()
            || TYPEOF(names) != SEXPTYPE::STRSXP
            || XLENGTH(names) < 2
        {
            return R_NilValue();
        }
        let lhs = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 0)))
            .to_string_lossy()
            .into_owned();
        let rhs = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 1)))
            .to_string_lossy()
            .into_owned();
        let text = format!("{lhs} ~ {rhs}");
        let s = Rf_mkString(std::ffi::CString::new(text).unwrap().as_ptr());
        let _g = protect(s);
        parse_formula_text(s)
    }
}

/// GNU `replications(~ a, data)` — balanced one-factor count.
pub unsafe fn do_replications(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let data = CAR(CDR(args));
        if data.is_null() || data == R_NilValue() {
            return R_NilValue();
        }
        let col = if TYPEOF(data) == SEXPTYPE::VECSXP && XLENGTH(data) > 0 {
            VECTOR_ELT(data, 0)
        } else {
            data
        };
        let n = XLENGTH(col) as usize;
        if n == 0 {
            return Rf_ScalarInteger(0);
        }
        let first = if TYPEOF(col) == SEXPTYPE::INTSXP {
            *INTEGER(col) as f64
        } else {
            *REAL(col)
        };
        let mut count = 0i32;
        for i in 0..n {
            let v = if TYPEOF(col) == SEXPTYPE::INTSXP {
                *INTEGER(col).add(i) as f64
            } else {
                *REAL(col).add(i)
            };
            if (v - first).abs() < 1e-12 {
                count += 1;
            }
        }
        Rf_ScalarInteger(count)
    }
}

/// GNU `.nknots.smspl(n)` — default smooth.spline knot count.
pub unsafe fn do_nknots_smspl(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if TYPEOF(n_s) == SEXPTYPE::INTSXP {
            *INTEGER(n_s)
        } else {
            *REAL(n_s) as i32
        };
        if n < 50 {
            return Rf_ScalarInteger(n);
        }
        let nf = n as f64;
        let a1 = 50f64.log2();
        let a2 = 100f64.log2();
        let a3 = 140f64.log2();
        let a4 = 200f64.log2();
        let knots = if n < 200 {
            2f64.powf(a1 + (a2 - a1) * (nf - 50.0) / 150.0)
        } else if n < 800 {
            2f64.powf(a2 + (a3 - a2) * (nf - 200.0) / 600.0)
        } else if n < 3200 {
            2f64.powf(a3 + (a4 - a3) * (nf - 800.0) / 2400.0)
        } else {
            200.0 + (nf - 3200.0).powf(0.2)
        };
        Rf_ScalarInteger(knots.trunc() as i32)
    }
}

/// GNU `.getXlevels(Terms, m)` — factor levels from the model frame.
pub unsafe fn do_get_xlevels(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = CAR(CDR(args));
        if m.is_null() || TYPEOF(m) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names = crate::sexp::attrib_core::getAttrib(
            m,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let mut out_cols: Vec<SEXP> = Vec::new();
        let mut out_names: Vec<String> = Vec::new();
        for j in 0..XLENGTH(m) {
            let col = VECTOR_ELT(m, j);
            let class = crate::sexp::attrib_core::getAttrib(
                col,
                crate::sexp::attrib_core::R_ClassSymbol(),
            );
            let mut is_fac = false;
            if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
                for i in 0..XLENGTH(class) {
                    if std::ffi::CStr::from_ptr(CHAR(STRING_ELT(class, i))).to_bytes() == b"factor" {
                        is_fac = true;
                        break;
                    }
                }
            }
            if !is_fac {
                continue;
            }
            let lev = crate::sexp::attrib_core::getAttrib(
                col,
                crate::sexp::symbol::Rf_install(c"levels".as_ptr()),
            );
            if lev.is_null() || TYPEOF(lev) != SEXPTYPE::STRSXP {
                continue;
            }
            out_cols.push(lev);
            if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP && j < XLENGTH(names) {
                out_names.push(
                    std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, j)))
                        .to_string_lossy()
                        .into_owned(),
                );
            } else {
                out_names.push(format!("V{}", j + 1));
            }
        }
        if out_cols.is_empty() {
            return R_NilValue();
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, out_cols.len() as i64);
        let _r = protect(result);
        for (i, col) in out_cols.iter().enumerate() {
            SET_VECTOR_ELT(result, i as i64, *col);
        }
        crate::mainutils::essentials::set_string_names(result, &out_names);
        result
    }
}


/// GNU `.MFclass(x)` — model-frame column class label.
pub unsafe fn do_mfclass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        let mut is_factor = false;
        let mut is_ordered = false;
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(class) {
                let s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(class, i)));
                if s.to_bytes() == b"ordered" {
                    is_ordered = true;
                }
                if s.to_bytes() == b"factor" {
                    is_factor = true;
                }
            }
        }
        let label = if TYPEOF(x) == SEXPTYPE::LGLSXP {
            "logical"
        } else if is_ordered {
            "ordered"
        } else if is_factor {
            "factor"
        } else if TYPEOF(x) == SEXPTYPE::STRSXP {
            "character"
        } else {
            let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
            let is_mat = !dim.is_null()
                && dim != R_NilValue()
                && TYPEOF(dim) == SEXPTYPE::INTSXP
                && XLENGTH(dim) >= 2;
            let numeric = TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(x) == SEXPTYPE::INTSXP;
            if is_mat && numeric {
                let ncol = *INTEGER(dim).add(1);
                let text = format!("nmatrix.{ncol}");
                return Rf_mkString(std::ffi::CString::new(text).unwrap().as_ptr());
            } else if numeric {
                "numeric"
            } else {
                "other"
            }
        };
        Rf_mkString(std::ffi::CString::new(label).unwrap().as_ptr())
    }
}

/// GNU `.checkMFClasses(cl, m)` — NULL when named types match.
pub unsafe fn do_check_mf_classes(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let cl = CAR(args);
        let m = CAR(CDR(args));
        if cl.is_null()
            || TYPEOF(cl) != SEXPTYPE::STRSXP
            || XLENGTH(cl) == 0
            || m.is_null()
            || TYPEOF(m) != SEXPTYPE::VECSXP
        {
            return R_NilValue();
        }
        let cl_names = crate::sexp::attrib_core::getAttrib(
            cl,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        if cl_names.is_null() || TYPEOF(cl_names) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        let m_names = crate::sexp::attrib_core::getAttrib(
            m,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        if m_names.is_null() || TYPEOF(m_names) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        for i in 0..XLENGTH(cl) {
            let want = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(cl, i)))
                .to_string_lossy()
                .into_owned();
            let cname = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(cl_names, i)))
                .to_string_lossy()
                .into_owned();
            for j in 0..XLENGTH(m_names) {
                let mn = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(m_names, j)))
                    .to_string_lossy()
                    .into_owned();
                if mn == cname {
                    let col = VECTOR_ELT(m, j);
                    let got = if TYPEOF(col) == SEXPTYPE::REALSXP
                        || TYPEOF(col) == SEXPTYPE::INTSXP
                    {
                        "numeric"
                    } else {
                        "other"
                    };
                    if got != want {
                        return R_NilValue();
                    }
                }
            }
        }
        R_NilValue()
    }
}

/// GNU `.vcov.aliased(aliased, vc)` — pad NA rows/cols for aliased coefs.
pub unsafe fn do_vcov_aliased(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let aliased = CAR(args);
        let vc = CAR(CDR(args));
        let p = XLENGTH(aliased) as usize;
        let mut keep: Vec<usize> = Vec::new();
        for i in 0..p {
            let a = if TYPEOF(aliased) == SEXPTYPE::LGLSXP {
                *LOGICAL(aliased).add(i) != 0
            } else if TYPEOF(aliased) == SEXPTYPE::INTSXP {
                *INTEGER(aliased).add(i) != 0
            } else {
                *REAL(aliased).add(i) != 0.0
            };
            if !a {
                keep.push(i);
            }
        }
        if keep.len() == p {
            return vc;
        }
        let result = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), p as i32, p as i32);
        let _r = protect(result);
        for i in 0..p * p {
            *REAL(result).add(i) = NA_REAL;
        }
        let k = keep.len();
        for (ii, &i) in keep.iter().enumerate() {
            for (jj, &j) in keep.iter().enumerate() {
                let src = if TYPEOF(vc) == SEXPTYPE::REALSXP {
                    *REAL(vc).add(ii + jj * k)
                } else {
                    *INTEGER(vc).add(ii + jj * k) as f64
                };
                *REAL(result).add(i + j * p) = src;
            }
        }
        result
    }
}

/// GNU `makepredictcall(var, call)` — return `call` unchanged.
pub unsafe fn do_makepredictcall(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { CAR(CDR(args)) }
}








/// GNU `as.formula(object)`.
pub unsafe fn do_as_formula(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_formula(call, op, args, rho) }
}

/// GNU `asOneSidedFormula(object)` — `~expr`.
pub unsafe fn do_as_one_sided_formula(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if inherits_formula(x) {
            return x;
        }
        if TYPEOF(x) == SEXPTYPE::LANGSXP {
            let op = CAR(x);
            let name = if !op.is_null() && TYPEOF(op) == SEXPTYPE::SYMSXP {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(op)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "~" {
                return mark_formula(x);
            }
        }
        let rhs = if TYPEOF(x) == SEXPTYPE::STRSXP && XLENGTH(x) > 0 {
            let raw = CHAR(STRING_ELT(x, 0));
            let lab = std::ffi::CStr::from_ptr(raw).to_string_lossy();
            let c = std::ffi::CString::new(lab.as_ref()).unwrap_or_default();
            crate::sexp::symbol::Rf_install(c.as_ptr())
        } else if TYPEOF(x) == SEXPTYPE::SYMSXP {
            x
        } else {
            return R_NilValue();
        };
        let tilde = crate::sexp::symbol::Rf_install(c"~".as_ptr());
        mark_formula(Rf_lang2(tilde, rhs))
    }
}

/// GNU `get_all_vars(formula, data)` — formula columns from `data`.
pub unsafe fn do_get_all_vars(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_model_frame(call, op, args, rho) }
}

fn pack_allnames_args(expr: SEXP, functions: bool) -> SEXP {
    unsafe {
        let unique = Rf_ScalarLogical(1);
        let maxn = Rf_ScalarInteger(-1);
        let funs = Rf_ScalarLogical(if functions { 1 } else { 0 });
        let a1 = Rf_cons(unique, R_NilValue());
        let _a1 = protect(a1);
        let a2 = Rf_cons(maxn, a1);
        let _a2 = protect(a2);
        let a3 = Rf_cons(funs, a2);
        let _a3 = protect(a3);
        let a4 = Rf_cons(expr, a3);
        let _a4 = protect(a4);
        a4
    }
}

/// GNU `all.vars(expr)` — `.Internal(all.names(..., functions=FALSE))`.
pub unsafe fn do_all_vars(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::list::do_allnames(
            call,
            std::ptr::null_mut(),
            pack_allnames_args(CAR(args), false),
            rho,
        )
    }
}

/// GNU `all.names(expr)` — `.Internal(all.names(..., functions=TRUE))`.
pub unsafe fn do_all_names(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::list::do_allnames(
            call,
            std::ptr::null_mut(),
            pack_allnames_args(CAR(args), true),
            rho,
        )
    }
}






/// GNU `model.extract(frame, component)` — `response` or `(component)`.
pub unsafe fn do_model_extract(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let frame = crate::eval::eval::Rf_eval(CAR(args), rho);
        let spec = CAR(CDR(args));
        let name = if TYPEOF(spec) == SEXPTYPE::SYMSXP {
            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(spec)))
                .to_string_lossy()
                .into_owned()
        } else if TYPEOF(spec) == SEXPTYPE::STRSXP && XLENGTH(spec) > 0 {
            std::ffi::CStr::from_ptr(CHAR(STRING_ELT(spec, 0)))
                .to_string_lossy()
                .into_owned()
        } else {
            String::new()
        };
        if name == "response" {
            if TYPEOF(frame) == SEXPTYPE::VECSXP && XLENGTH(frame) > 0 {
                return VECTOR_ELT(frame, 0);
            }
            return R_NilValue();
        }
        if name == "offset" {
            return named_list_elt(frame, "offset");
        }
        let key = format!("({name})");
        named_list_elt(frame, &key)
    }
}


fn mark_terms(form: SEXP, response: i32) -> SEXP {
    unsafe {
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _cl = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"terms".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"formula".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            form,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        crate::sexp::attrib_core::setAttrib(
            form,
            crate::sexp::symbol::Rf_install(c"response".as_ptr()),
            Rf_ScalarInteger(response),
        );
        crate::sexp::attrib_core::setAttrib(
            form,
            crate::sexp::symbol::Rf_install(c"intercept".as_ptr()),
            Rf_ScalarInteger(1),
        );
        let mut labels = Vec::new();
        collect_term_labels(form, &mut labels);
        if response > 0 && !labels.is_empty() {
            labels.remove(0);
        }
        let lab = Rf_allocVector3(SEXPTYPE::STRSXP, labels.len() as i64);
        let _lb = protect(lab);
        for (i, name) in labels.iter().enumerate() {
            let c = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(lab, i as i64, Rf_mkChar(c.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            form,
            crate::sexp::symbol::Rf_install(c"term.labels".as_ptr()),
            lab,
        );
        let mut vars = Vec::new();
        for lab in &labels {
            for part in lab.split(':') {
                if !vars.iter().any(|s| s == part) {
                    vars.push(part.to_string());
                }
            }
        }
        let nrows = vars.len() + if response > 0 { 1 } else { 0 };
        let ncols = labels.len();
        if ncols > 0 && nrows > 0 {
            let fac = crate::mainutils::array::allocMatrix(
                SEXPTYPE::INTSXP.as_c_int(),
                nrows as i32,
                ncols as i32,
            );
            let _f = protect(fac);
            for i in 0..nrows * ncols {
                *INTEGER(fac).add(i) = 0;
            }
            let row0 = if response > 0 { 1 } else { 0 };
            for (j, lab) in labels.iter().enumerate() {
                let parts: Vec<&str> = lab.split(':').collect();
                for (i, var) in vars.iter().enumerate() {
                    if parts.iter().any(|p| *p == var) {
                        *INTEGER(fac).add(row0 + i + j * nrows) = 1;
                    }
                }
            }
            let rn = Rf_allocVector3(SEXPTYPE::STRSXP, nrows as i64);
            let _rn = protect(rn);
            let mut r = 0;
            if response > 0 {
                SET_STRING_ELT(rn, 0, Rf_mkChar(c"y".as_ptr()));
                r = 1;
            }
            for var in &vars {
                let c = std::ffi::CString::new(var.as_str()).unwrap_or_default();
                SET_STRING_ELT(rn, r, Rf_mkChar(c.as_ptr()));
                r += 1;
            }
            let cn = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as i64);
            let _cn = protect(cn);
            for (j, lab) in labels.iter().enumerate() {
                let c = std::ffi::CString::new(lab.as_str()).unwrap_or_default();
                SET_STRING_ELT(cn, j as i64, Rf_mkChar(c.as_ptr()));
            }
            let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            let _dn = protect(dn);
            SET_VECTOR_ELT(dn, 0, rn);
            SET_VECTOR_ELT(dn, 1, cn);
            crate::sexp::attrib_core::setAttrib(
                fac,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dn,
            );
            crate::sexp::attrib_core::setAttrib(
                form,
                crate::sexp::symbol::Rf_install(c"factors".as_ptr()),
                fac,
            );
        }
        form
    }
}

fn matrix_colnames(x: SEXP) -> Vec<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP {
            let mut out = Vec::new();
            for i in 0..XLENGTH(x) {
                out.push(
                    std::ffi::CStr::from_ptr(CHAR(STRING_ELT(x, i)))
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            return out;
        }
        let labs = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"term.labels".as_ptr()),
        );
        if !labs.is_null() && labs != R_NilValue() && TYPEOF(labs) == SEXPTYPE::STRSXP {
            return matrix_colnames(labs);
        }
        let dn = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimNamesSymbol());
        if !dn.is_null() && dn != R_NilValue() && TYPEOF(dn) == SEXPTYPE::VECSXP && XLENGTH(dn) >= 2 {
            return matrix_colnames(VECTOR_ELT(dn, 1));
        }
        Vec::new()
    }
}

/// GNU `factor.scope(factor, scope)` — add terms not already in the model.
pub unsafe fn do_factor_scope(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let factor = CAR(args);
        let scope = CAR(CDR(args));
        let add = named_list_elt(scope, "add");
        let have = matrix_colnames(factor);
        let extra: Vec<String> = matrix_colnames(add)
            .into_iter()
            .filter(|n| !have.iter().any(|h| h == n))
            .collect();
        let add_out = Rf_allocVector3(SEXPTYPE::STRSXP, extra.len() as i64);
        let _a = protect(add_out);
        for (i, name) in extra.iter().enumerate() {
            let c = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(add_out, i as i64, Rf_mkChar(c.as_ptr()));
        }
        let drop_out = Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        let _d = protect(drop_out);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, drop_out);
        SET_VECTOR_ELT(result, 1, add_out);
        crate::mainutils::essentials::set_string_names(
            result,
            &["drop".to_string(), "add".to_string()],
        );
        result
    }
}


/// GNU `terms(object)` — `$terms`, or a formula as `c("terms","formula")`.
pub unsafe fn do_terms(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let t = named_list_elt(x, "terms");
        if !t.is_null() && t != R_NilValue() {
            return t;
        }
        if TYPEOF(x) == SEXPTYPE::LANGSXP || inherits_formula(x) {
            let rest = CDR(x);
            let third = if rest.is_null() {
                R_NilValue()
            } else {
                CDR(rest)
            };
            let response = if third.is_null() || third == R_NilValue() {
                0
            } else {
                1
            };
            return mark_terms(x, response);
        }
        R_NilValue()
    }
}

/// GNU `delete.response(termobj)` — drop the LHS of a two-sided formula.
pub unsafe fn do_delete_response(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::LANGSXP {
            return x;
        }
        let rest = CDR(x);
        let third = if rest.is_null() {
            R_NilValue()
        } else {
            CDR(rest)
        };
        if third.is_null() || third == R_NilValue() {
            return mark_terms(x, 0);
        }
        let tilde = crate::sexp::symbol::Rf_install(c"~".as_ptr());
        mark_terms(Rf_lang2(tilde, CAR(third)), 0)
    }
}

/// GNU `drop.terms(termobj, dropx)` — drop term.labels[dropx].
pub unsafe fn do_drop_terms(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let drop = CAR(CDR(args));
        let drop_i = if drop.is_null() || drop == R_NilValue() {
            0
        } else if TYPEOF(drop) == SEXPTYPE::INTSXP {
            *INTEGER(drop)
        } else if TYPEOF(drop) == SEXPTYPE::REALSXP {
            *REAL(drop) as i32
        } else {
            0
        };
        let labs = crate::sexp::attrib_core::getAttrib(
            obj,
            crate::sexp::symbol::Rf_install(c"term.labels".as_ptr()),
        );
        if labs.is_null() || labs == R_NilValue() || TYPEOF(labs) != SEXPTYPE::STRSXP {
            return do_delete_response(call, op, args, rho);
        }
        let n = XLENGTH(labs);
        let keep_n = if drop_i >= 1 && (drop_i as i64) <= n {
            n - 1
        } else {
            n
        };
        let kept = Rf_allocVector3(SEXPTYPE::STRSXP, keep_n);
        let _k = protect(kept);
        let mut j = 0i64;
        for i in 0..n {
            if drop_i >= 1 && i + 1 == drop_i as i64 {
                continue;
            }
            SET_STRING_ELT(kept, j, STRING_ELT(labs, i));
            j += 1;
        }
        let form = do_reformulate(
            call,
            op,
            Rf_cons(kept, R_NilValue()),
            rho,
        );
        let _f = protect(form);
        mark_terms(form, 0)
    }
}

fn term_label_strings(obj: SEXP) -> Vec<String> {
    unsafe {
        let labs = crate::sexp::attrib_core::getAttrib(
            obj,
            crate::sexp::symbol::Rf_install(c"term.labels".as_ptr()),
        );
        let mut out = Vec::new();
        if labs.is_null() || labs == R_NilValue() || TYPEOF(labs) != SEXPTYPE::STRSXP {
            return out;
        }
        for i in 0..XLENGTH(labs) {
            let raw = CHAR(STRING_ELT(labs, i));
            if raw.is_null() {
                continue;
            }
            out.push(std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned());
        }
        out
    }
}

/// GNU `drop.scope(terms1)` — `term.labels` that can be dropped.
pub unsafe fn do_drop_scope(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let t1 = do_terms(call, op, args, rho);
        let labels = term_label_strings(t1);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, labels.len() as i64);
        let _o = protect(out);
        for (i, name) in labels.iter().enumerate() {
            let c = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(out, i as i64, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}

/// GNU `add.scope(terms1, terms2)` — labels in 2 not in 1.
pub unsafe fn do_add_scope(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let t1 = do_terms(call, op, Rf_cons(CAR(args), R_NilValue()), rho);
        let t2 = do_terms(call, op, Rf_cons(CAR(CDR(args)), R_NilValue()), rho);
        let a = term_label_strings(t1);
        let b = term_label_strings(t2);
        let extra: Vec<String> = b
            .into_iter()
            .filter(|s| !a.iter().any(|t| t == s))
            .collect();
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, extra.len() as i64);
        let _o = protect(out);
        for (i, name) in extra.iter().enumerate() {
            let c = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(out, i as i64, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}




/// GNU `offset(object)` is identity.
pub unsafe fn do_offset(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { CAR(args) }
}

/// GNU default `getCall(x)` via $call.
pub unsafe fn do_getCall(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { named_list_elt(CAR(args), "call") }
}

/// GNU `contr.treatment(n, base=1)`.
pub unsafe fn do_contr_treatment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if TYPEOF(n_s) == SEXPTYPE::INTSXP {
            *INTEGER(n_s)
        } else {
            *REAL(n_s) as c_int
        };
        let mut base = 1i32;
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let b = CAR(rest);
            if TYPEOF(b) == SEXPTYPE::INTSXP {
                base = *INTEGER(b);
            } else if TYPEOF(b) == SEXPTYPE::REALSXP {
                base = *REAL(b) as c_int;
            }
        }
        if n < 2 || base < 1 || base > n {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid contrasts",
            );
        }
        let nc = (n - 1) as i32;
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n, nc);
        let _m = protect(mat);
        for i in 0..(n as usize * nc as usize) {
            *REAL(mat).add(i) = 0.0;
        }
        let mut col = 0usize;
        for lev in 1..=n {
            if lev == base {
                continue;
            }
            *REAL(mat).add((lev as usize - 1) + col * n as usize) = 1.0;
            col += 1;
        }
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, n as i64);
        let _rn = protect(rn);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, nc as i64);
        let _cn = protect(cn);
        for i in 0..n as usize {
            let lab = format!("{}\0", i + 1);
            SET_STRING_ELT(rn, i as i64, Rf_mkChar(lab.as_ptr() as *const _));
        }
        col = 0;
        for lev in 1..=n {
            if lev == base {
                continue;
            }
            let lab = format!("{lev}\0");
            SET_STRING_ELT(cn, col as i64, Rf_mkChar(lab.as_ptr() as *const _));
            col += 1;
        }
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(
            mat,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dn,
        );
        mat
    }
}

/// GNU `contr.sum(n)`.
pub unsafe fn do_contr_sum(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if TYPEOF(n_s) == SEXPTYPE::INTSXP {
            *INTEGER(n_s)
        } else {
            *REAL(n_s) as c_int
        };
        if n < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid contrasts",
            );
        }
        let nc = n - 1;
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n, nc);
        let _m = protect(mat);
        for i in 0..(n as usize * nc as usize) {
            *REAL(mat).add(i) = 0.0;
        }
        for j in 0..nc as usize {
            *REAL(mat).add(j + j * n as usize) = 1.0;
            *REAL(mat).add((n as usize - 1) + j * n as usize) = -1.0;
        }
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, n as i64);
        let _rn = protect(rn);
        for i in 0..n as usize {
            let lab = format!("{}\0", i + 1);
            SET_STRING_ELT(rn, i as i64, Rf_mkChar(lab.as_ptr() as *const _));
        }
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, R_NilValue());
        crate::sexp::attrib_core::setAttrib(
            mat,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dn,
        );
        mat
    }
}

/// GNU `contr.helmert(n)`.
pub unsafe fn do_contr_helmert(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if TYPEOF(n_s) == SEXPTYPE::INTSXP {
            *INTEGER(n_s)
        } else {
            *REAL(n_s) as c_int
        };
        if n < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid contrasts",
            );
        }
        let nc = n - 1;
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n, nc);
        let _m = protect(mat);
        for i in 0..(n as usize * nc as usize) {
            *REAL(mat).add(i) = 0.0;
        }
        for j in 0..nc as usize {
            for i in 0..=j {
                *REAL(mat).add(i + j * n as usize) = -1.0;
            }
            *REAL(mat).add((j + 1) + j * n as usize) = (j + 1) as f64;
        }
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, n as i64);
        let _rn = protect(rn);
        for i in 0..n as usize {
            let lab = format!("{}\0", i + 1);
            SET_STRING_ELT(rn, i as i64, Rf_mkChar(lab.as_ptr() as *const _));
        }
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, R_NilValue());
        crate::sexp::attrib_core::setAttrib(
            mat,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dn,
        );
        mat
    }
}

/// GNU `contr.poly(n)` — orthonormal polynomials on scores `1:n`.
pub unsafe fn do_contr_poly(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if TYPEOF(n_s) == SEXPTYPE::INTSXP {
            *INTEGER(n_s)
        } else {
            *REAL(n_s) as c_int
        } as usize;
        if n < 2 {
            return R_NilValue();
        }
        let scores: Vec<f64> = (1..=n).map(|i| i as f64).collect();
        let mean = scores.iter().sum::<f64>() / n as f64;
        let y: Vec<f64> = scores.iter().map(|s| s - mean).collect();
        let mut cols: Vec<Vec<f64>> = (0..n)
            .map(|k| y.iter().map(|v| v.powi(k as i32)).collect())
            .collect();
        for j in 0..n {
            for i in 0..j {
                let dot: f64 = cols[j].iter().zip(&cols[i]).map(|(a, b)| a * b).sum();
                let nrm: f64 = cols[i].iter().map(|a| a * a).sum();
                if nrm > 0.0 {
                    let c = dot / nrm;
                    for k in 0..n {
                        cols[j][k] -= c * cols[i][k];
                    }
                }
            }
            let nrm = cols[j].iter().map(|a| a * a).sum::<f64>().sqrt();
            if nrm > 0.0 {
                for k in 0..n {
                    cols[j][k] /= nrm;
                }
            }
            // GNU QR keeps the first nonzero entry of each contrast matching
            // the sign of the raw monomial (linear starts negative).
            if let Some(&first) = cols[j].iter().find(|v| v.abs() > 1e-12)
                && first > 0.0
                && j == 1
            {
                for k in 0..n {
                    cols[j][k] = -cols[j][k];
                }
            }
        }
        let nc = n - 1;
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, nc as i32);
        let _m = protect(mat);
        for j in 0..nc {
            for i in 0..n {
                *REAL(mat).add(i + j * n) = cols[j + 1][i];
            }
        }
        mat
    }
}


/// GNU `contr.SAS(n)` is treatment with last level as base.
pub unsafe fn do_contr_sas(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if TYPEOF(n_s) == SEXPTYPE::INTSXP {
            *INTEGER(n_s)
        } else {
            *REAL(n_s) as c_int
        };
        let base = Rf_ScalarInteger(n);
        let _b = protect(base);
        let newargs = Rf_cons(n_s, Rf_cons(base, R_NilValue()));
        let _a = protect(newargs);
        do_contr_treatment(call, op, newargs, rho)
    }
}

/// GNU `contrasts(factor)` default treatment coding.
pub unsafe fn do_contrasts(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        let stored = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"contrasts".as_ptr()),
        );
        if !stored.is_null() && stored != R_NilValue() {
            return stored;
        }
        let levels = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_LevelsSymbol(),
        );
        let n = if !levels.is_null()
            && levels != R_NilValue()
            && TYPEOF(levels) == SEXPTYPE::STRSXP
        {
            XLENGTH(levels) as c_int
        } else {
            0
        };
        if n < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "contrasts apply only to factors with 2 or more levels",
            );
        }
        let n_s = Rf_ScalarInteger(n);
        let _ns = protect(n_s);
        let targs = Rf_cons(n_s, R_NilValue());
        let _ta = protect(targs);
        let mat = do_contr_treatment(call, op, targs, rho);
        let _m = protect(mat);
        let nc = n - 1;
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, n as i64);
        let _rn = protect(rn);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, nc as i64);
        let _cn = protect(cn);
        for i in 0..n as i64 {
            SET_STRING_ELT(rn, i, STRING_ELT(levels, i));
        }
        for i in 0..nc as i64 {
            SET_STRING_ELT(cn, i, STRING_ELT(levels, i + 1));
        }
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(
            mat,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dn,
        );
        mat
    }
}

/// GNU `contrasts(x) <- value` — store the contrast matrix.
pub unsafe fn do_contrasts_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let val = CAR(CDR(args));
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"contrasts".as_ptr()),
            val,
        );
        x
    }
}


/// GNU `C(factor)` attaches the default contrast name.
pub unsafe fn do_C(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let object = crate::mainutils::duplicate::Rf_duplicate(CAR(args));
        let ordered =
            crate::mainutils::objects::inherits2(object, c"ordered".as_ptr()) != FALSE;
        let (kind, name) = if ordered {
            (c"contr.poly".as_ptr(), c"ordered".as_ptr())
        } else {
            (c"contr.treatment".as_ptr(), c"unordered".as_ptr())
        };
        let contr = Rf_mkString(kind);
        let _c = protect(contr);
        let names = Rf_mkString(name);
        let _n = protect(names);
        crate::sexp::attrib_core::setAttrib(
            contr,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        crate::sexp::attrib_core::setAttrib(
            object,
            crate::sexp::symbol::Rf_install(c"contrasts".as_ptr()),
            contr,
        );
        object
    }
}






















