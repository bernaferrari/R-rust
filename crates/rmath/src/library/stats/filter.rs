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
        let n = XLENGTH(x);
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










