//! Misc ts utilities: spectra, decompose, ARMAacf/ARMAtoMA, kernels, stepfun, na.action

use super::*;

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
        let freq = if !tsp.is_null() && TYPEOF(tsp) == SEXPTYPE::REALSXP && XLENGTH(tsp) >= 3 {
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
            if i < half || i + half >= n {
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
                *REAL(ans).add(i) = phi * *REAL(ans).add(i - 1) + phi2 * *REAL(ans).add(i - 2);
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

/// GNU `C_ARMAtoMA(ar, ma, lag.max)` — MA(∞) coefficients, ported from pacf.c.
pub unsafe fn ARMAtoMA(ar: SEXP, ma: SEXP, lag_max: SEXP) -> SEXP {
    unsafe {
        let p = crate::sexp::accessors::LENGTH(ar) as usize;
        let q = crate::sexp::accessors::LENGTH(ma) as usize;
        let m = crate::mainutils::coerce::asInteger(lag_max);
        if m <= 0 || m == crate::sexp::ffi::NA_INTEGER {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid value of lag.max",
            );
        }
        let phi = crate::sexp::accessors::REAL(ar);
        let theta = crate::sexp::accessors::REAL(ma);
        let res = crate::sexp::constructors::Rf_allocVector3(
            crate::sexp::ffi::SEXPTYPE::REALSXP,
            m as i64,
        );
        let _r = protect(res);
        let psi = crate::sexp::accessors::REAL(res);
        for i in 0..m as usize {
            let mut tmp = if i < q { *theta.add(i) } else { 0.0 };
            for j in 0..((i + 1).min(p)) {
                tmp += *phi.add(j) * if i > j { *psi.add(i - j - 1) } else { 1.0 };
            }
            *psi.add(i) = tmp;
        }
        res
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
        let mat =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), p as i32, p as i32);
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
        crate::sexp::attrib_core::setAttrib(mat, crate::sexp::attrib_core::R_DimNamesSymbol(), dn);
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
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
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
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
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
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
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
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
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
        crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"na.action".as_ptr()),
        )
    }
}

/// GNU `model.weights(x)` — `x[["(weights)"]]`.
pub unsafe fn do_model_weights(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { named_list_elt(CAR(args), "(weights)") }
}
