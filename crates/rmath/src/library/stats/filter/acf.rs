//! Autocorrelation family (acf, pacf, ccf)

use super::*;

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
pub unsafe extern "C-unwind" fn c_acf(x: SEXP, lmax: SEXP, sCor: SEXP) -> SEXP {
    unsafe { acf(x, lmax, sCor) }
}
pub unsafe extern "C-unwind" fn c_pacf1(acf: SEXP, lmax: SEXP) -> SEXP {
    unsafe {
        let lagmax = as_integer(lmax);
        if lagmax < 1 {
            return Rf_allocVector(SEXPTYPE::REALSXP, 0);
        }
        let acf = coerceVector(acf, SEXPTYPE::REALSXP.as_c_int());
        let _acf = protect(acf);
        let ans = Rf_allocVector(SEXPTYPE::REALSXP, lagmax);
        let _ans = protect(ans);
        let cor = REAL(acf);
        let p = REAL(ans);
        let nlag = lagmax as usize;
        let mut w = vec![0.0f64; nlag];
        let mut v = vec![0.0f64; nlag];
        w[0] = *cor.add(1);
        *p = w[0];
        for ll in 1..nlag {
            let mut a = *cor.add(ll + 1);
            let mut b = 1.0;
            for i in 0..ll {
                a -= w[i] * *cor.add(ll - i);
                b -= w[i] * *cor.add(i + 1);
            }
            let c = a / b;
            *p.add(ll) = c;
            if ll + 1 == nlag {
                break;
            }
            w[ll] = c;
            for i in 0..ll {
                v[ll - i - 1] = w[i];
            }
            for i in 0..ll {
                w[i] -= c * v[i];
            }
        }
        let d = Rf_allocVector(SEXPTYPE::INTSXP, 3);
        let _d = protect(d);
        *INTEGER(d) = lagmax;
        *INTEGER(d).add(1) = 1;
        *INTEGER(d).add(2) = 1;
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
