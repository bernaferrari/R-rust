//! Linear-model infrastructure (reorder, oneway, loglik/AIC, confint, influence, lm internals) — extracted verbatim from essentials/mathstats.rs.

use super::*;

/// GNU `reorder(x, X)` by group means.
pub unsafe fn do_reorder(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x0 = CAR(args);
        if class_contains(x0, "dendrogram") {
            return do_reorder_dendrogram(_call, _op, args, _rho);
        }
        let mut ans = R_NilValue();
        if crate::eval::dispatch::DispatchOrEval(
            _call,
            _op,
            c"reorder".as_ptr(),
            args,
            _rho,
            &mut ans,
            0,
            1,
        ) != 0
        {
            return ans;
        }
        let x = CAR(args);
        let xx = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || xx.is_null() || xx == R_NilValue() {
            return R_NilValue();
        }
        let levels =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_LevelsSymbol());
        if levels.is_null() || levels == R_NilValue() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        let nlev = XLENGTH(levels) as usize;
        let n = XLENGTH(x) as usize;
        let mut codes = vec![0i32; n];
        for i in 0..n {
            codes[i] = if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(x).add(i)
            } else {
                elt_real_safe(x, i as i64).round() as i32
            };
        }
        let mut sums = vec![0.0; nlev];
        let mut cnt = vec![0.0; nlev];
        for i in 0..n {
            let g = codes[i];
            if g >= 1 && (g as usize) <= nlev {
                sums[(g as usize) - 1] += elt_real_safe(xx, i as i64);
                cnt[(g as usize) - 1] += 1.0;
            }
        }
        let mut scores = vec![0.0; nlev];
        let mut order: Vec<usize> = (0..nlev).collect();
        for i in 0..nlev {
            scores[i] = if cnt[i] > 0.0 {
                sums[i] / cnt[i]
            } else {
                f64::NAN
            };
        }
        order.sort_by(|&a, &b| {
            scores[a]
                .partial_cmp(&scores[b])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut new_code = vec![0i32; nlev];
        for (new_i, &old) in order.iter().enumerate() {
            new_code[old] = (new_i as i32) + 1;
        }
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n as i64);
        let _r = protect(result);
        for i in 0..n {
            let g = codes[i];
            *INTEGER(result).add(i) = if g >= 1 && (g as usize) <= nlev {
                new_code[(g as usize) - 1]
            } else {
                NA_INTEGER
            };
        }
        let new_levels = Rf_allocVector3(SEXPTYPE::STRSXP, nlev as i64);
        let _nl = protect(new_levels);
        for (new_i, &old) in order.iter().enumerate() {
            SET_STRING_ELT(new_levels, new_i as i64, STRING_ELT(levels, old as i64));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_LevelsSymbol(),
            new_levels,
        );
        let class = Rf_mkString(c"factor".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        let sc = Rf_allocVector3(SEXPTYPE::REALSXP, nlev as i64);
        let _sc = protect(sc);
        for i in 0..nlev {
            *REAL(sc).add(i) = scores[i];
        }
        crate::sexp::attrib_core::setAttrib(sc, crate::sexp::attrib_core::R_NamesSymbol(), levels);
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"scores".as_ptr()), sc);
        result
    }
}

/// GNU `oneway.test(x ~ g)` / `oneway.test(x, g)`.
pub unsafe fn do_oneway_test(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let first = CAR(args);
        let mut x = first;
        let mut g = CAR(CDR(args));
        let mut var_equal = false;
        if !first.is_null() && first != R_NilValue() && TYPEOF(first) == SEXPTYPE::LANGSXP {
            let lhs = CADR(first);
            let rhs = CAR(CDR(CDR(first)));
            if !lhs.is_null() && lhs != R_NilValue() {
                x = crate::eval::eval::Rf_eval(lhs, rho);
            }
            if !rhs.is_null() && rhs != R_NilValue() {
                g = crate::eval::eval::Rf_eval(rhs, rho);
            }
        }
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
            if name == "var.equal" {
                let v = CAR(cell);
                if TYPEOF(v) == SEXPTYPE::LGLSXP && XLENGTH(v) > 0 {
                    var_equal = *LOGICAL(v) == TRUE;
                } else if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                    var_equal = *INTEGER(v) != 0;
                }
            }
            cell = CDR(cell);
        }
        if x.is_null() || x == R_NilValue() || g.is_null() || g == R_NilValue() {
            return R_NilValue();
        }
        let n0 = XLENGTH(x).min(XLENGTH(g));
        let mut pairs: Vec<(i32, f64)> = Vec::new();
        for i in 0..n0 {
            let gi = elt_real_safe(g, i);
            let xi = elt_real_safe(x, i);
            if gi.is_finite() && xi.is_finite() {
                pairs.push((gi.round() as i32, xi));
            }
        }
        let mut levels: Vec<i32> = pairs.iter().map(|p| p.0).collect();
        levels.sort_unstable();
        levels.dedup();
        let k = levels.len();
        if k < 2 {
            return R_NilValue();
        }
        let mut ns = vec![0.0; k];
        let mut means = vec![0.0; k];
        let mut vars = vec![0.0; k];
        let mut all = Vec::new();
        for (li, &lev) in levels.iter().enumerate() {
            let vs: Vec<f64> = pairs.iter().filter(|p| p.0 == lev).map(|p| p.1).collect();
            let n = vs.len() as f64;
            ns[li] = n;
            let m = vs.iter().sum::<f64>() / n;
            means[li] = m;
            vars[li] = vs.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (n - 1.0);
            all.extend(vs);
        }
        let (stat, df1, df2, method) = if var_equal {
            let n = all.len() as f64;
            let grand = all.iter().sum::<f64>() / n;
            let ssb: f64 = ns
                .iter()
                .zip(means.iter())
                .map(|(ni, mi)| ni * (mi - grand) * (mi - grand))
                .sum();
            let ssw: f64 = ns
                .iter()
                .zip(vars.iter())
                .map(|(ni, vi)| (ni - 1.0) * vi)
                .sum();
            let df1 = (k - 1) as f64;
            let df2 = n - k as f64;
            let stat = (ssb / df1) / (ssw / df2);
            (stat, df1, df2, "One-way analysis of means")
        } else {
            let w: Vec<f64> = ns.iter().zip(vars.iter()).map(|(n, v)| n / v).collect();
            let sum_w: f64 = w.iter().sum();
            let m = w
                .iter()
                .zip(means.iter())
                .map(|(wi, mi)| wi * mi)
                .sum::<f64>()
                / sum_w;
            let tmp: f64 = w
                .iter()
                .zip(ns.iter())
                .map(|(wi, ni)| {
                    let t = 1.0 - wi / sum_w;
                    t * t / (ni - 1.0)
                })
                .sum::<f64>()
                / ((k * k - 1) as f64);
            let stat = w
                .iter()
                .zip(means.iter())
                .map(|(wi, mi)| wi * (mi - m) * (mi - m))
                .sum::<f64>()
                / ((k as f64 - 1.0) * (1.0 + 2.0 * (k as f64 - 2.0) * tmp));
            let df1 = (k - 1) as f64;
            let df2 = 1.0 / (3.0 * tmp);
            (
                stat,
                df1,
                df2,
                "One-way analysis of means (not assuming equal variances)",
            )
        };
        let pval = crate::dist::f_dist::pf_inner(stat, df1, df2, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["F".to_string()]);
        let parameter = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _pa = protect(parameter);
        *REAL(parameter) = df1;
        *REAL(parameter).add(1) = df2;
        set_string_names(parameter, &["num df".to_string(), "denom df".to_string()]);
        let method_s = if var_equal {
            Rf_mkString(c"One-way analysis of means".as_ptr())
        } else {
            Rf_mkString(c"One-way analysis of means (not assuming equal variances)".as_ptr())
        };
        let _ = method;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, method_s);
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x and g".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "method".to_string(),
                "data.name".to_string(),
            ],
        );
        let class = Rf_mkString(c"htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn loglik_numeric(x: SEXP) -> f64 {
    unsafe { elt_real_safe(x, 0) }
}

fn loglik_df(x: SEXP) -> f64 {
    unsafe {
        let df = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"df".as_ptr()));
        if !df.is_null() && df != R_NilValue() {
            elt_real_safe(df, 0)
        } else {
            0.0
        }
    }
}

fn loglik_nobs(x: SEXP) -> f64 {
    unsafe {
        let n = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"nobs".as_ptr()));
        if !n.is_null() && n != R_NilValue() {
            elt_real_safe(n, 0)
        } else {
            0.0
        }
    }
}

/// GNU `AIC(object, k=2)` for a `logLik`.
pub unsafe fn do_aic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mut k = 2.0;
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
            if name == "k" || name.is_empty() {
                let v = CAR(cell);
                if !v.is_null() && v != R_NilValue() {
                    let kv = elt_real_safe(v, 0);
                    if kv.is_finite() {
                        k = kv;
                    }
                }
            }
            cell = CDR(cell);
        }
        Rf_ScalarReal(-2.0 * loglik_numeric(x) + k * loglik_df(x))
    }
}

/// GNU `BIC(object)` for a `logLik`.
pub unsafe fn do_bic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = loglik_nobs(x);
        Rf_ScalarReal(-2.0 * loglik_numeric(x) + loglik_df(x) * n.ln())
    }
}

/// GNU `logLik` identity for an already-tagged logLik.
pub unsafe fn do_loglik(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { CAR(args) }
}

pub(super) unsafe fn list_named_elt(list: SEXP, name: &str) -> SEXP {
    unsafe {
        if list.is_null() || list == R_NilValue() || TYPEOF(list) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names =
            crate::sexp::attrib_core::getAttrib(list, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        for i in 0..XLENGTH(names) {
            let s = STRING_ELT(names, i);
            if s.is_null() {
                continue;
            }
            let raw = CHAR(s);
            if raw.is_null() {
                continue;
            }
            if std::ffi::CStr::from_ptr(raw).to_string_lossy() == name {
                return VECTOR_ELT(list, i);
            }
        }
        R_NilValue()
    }
}

/// GNU `sigma(object)` for a list with deviance, nobs, coefficients.
pub unsafe fn do_sigma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dev = list_named_elt(x, "deviance");
        let nobs = list_named_elt(x, "nobs");
        let coef = list_named_elt(x, "coefficients");
        if dev == R_NilValue() || nobs == R_NilValue() {
            return R_NilValue();
        }
        let d = elt_real_safe(dev, 0);
        let n = elt_real_safe(nobs, 0);
        let p = if coef == R_NilValue() {
            0.0
        } else {
            XLENGTH(coef) as f64
        };
        let den = n - p;
        if den <= 0.0 {
            return Rf_ScalarReal(f64::NAN);
        }
        Rf_ScalarReal((d / den).sqrt())
    }
}

/// GNU `extractAIC(fit, k)` glm-style: `c(edf, aic + (k-2)*edf)`.
pub unsafe fn do_extract_aic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fit = CAR(args);
        let mut k = 2.0;
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
            if name == "k" {
                let kv = elt_real_safe(CAR(cell), 0);
                if kv.is_finite() {
                    k = kv;
                }
            }
            cell = CDR(cell);
        }
        let resid = list_named_elt(fit, "residuals");
        let dfr = list_named_elt(fit, "df.residual");
        let aic = list_named_elt(fit, "aic");
        if resid == R_NilValue() || dfr == R_NilValue() || aic == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid) as f64;
        let edf = n - elt_real_safe(dfr, 0);
        let a = elt_real_safe(aic, 0);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _r = protect(result);
        *REAL(result) = edf;
        *REAL(result).add(1) = a + (k - 2.0) * edf;
        result
    }
}

/// GNU `case.names(object)` — `names(residuals)` or `1:n`.
pub unsafe fn do_case_names(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let r = list_named_elt(obj, "residuals");
        if r != R_NilValue() {
            let names =
                crate::sexp::attrib_core::getAttrib(r, crate::sexp::attrib_core::R_NamesSymbol());
            if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && XLENGTH(names) == XLENGTH(r)
            {
                return names;
            }
            let n = XLENGTH(r);
            let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _o = protect(out);
            for i in 0..n {
                let s = CString::new((i + 1).to_string()).unwrap_or_default();
                SET_STRING_ELT(out, i, Rf_mkChar(s.as_ptr()));
            }
            return out;
        }
        crate::mainutils::essentials::do_rownames(call, op, args, rho)
    }
}

/// GNU `variable.names(object)` — `names(coefficients)`.
pub unsafe fn do_variable_names(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let c = list_named_elt(obj, "coefficients");
        if c != R_NilValue() {
            let names =
                crate::sexp::attrib_core::getAttrib(c, crate::sexp::attrib_core::R_NamesSymbol());
            if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
                return names;
            }
        }
        crate::mainutils::essentials::do_colnames(call, op, args, rho)
    }
}

/// GNU `confint(object)` from `$coefficients` and `$vcov`.
pub unsafe fn do_confint(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let mut level = 0.95;
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
            if name == "level" {
                let v = elt_real_safe(CAR(cell), 0);
                if v.is_finite() && v > 0.0 && v < 1.0 {
                    level = v;
                }
            }
            cell = CDR(cell);
        }
        let cf = list_named_elt(obj, "coefficients");
        let vcov = list_named_elt(obj, "vcov");
        if cf == R_NilValue() || vcov == R_NilValue() {
            return R_NilValue();
        }
        let p = XLENGTH(cf) as usize;
        if p == 0 {
            return R_NilValue();
        }
        let a = (1.0 - level) / 2.0;
        let zlo = crate::dist::normal::qnorm5_inner(a, 0.0, 1.0, true, false);
        let zhi = crate::dist::normal::qnorm5_inner(1.0 - a, 0.0, 1.0, true, false);
        let result =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), p as i32, 2);
        let _r = protect(result);
        for i in 0..p {
            let est = elt_real_safe(cf, i as i64);
            let var = elt_real_safe(vcov, (i + i * p) as i64);
            let se = var.max(0.0).sqrt();
            *REAL(result).add(i) = est + se * zlo;
            *REAL(result).add(i + p) = est + se * zhi;
        }
        let rn = crate::sexp::attrib_core::getAttrib(cf, crate::sexp::attrib_core::R_NamesSymbol());
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _cn = protect(cn);
        let lo_pct = format!("{} %", (100.0 * a * 10.0).round() / 10.0);
        let hi_pct = format!("{} %", (100.0 * (1.0 - a) * 10.0).round() / 10.0);
        let lo_c = std::ffi::CString::new(lo_pct).unwrap_or_default();
        let hi_c = std::ffi::CString::new(hi_pct).unwrap_or_default();
        SET_STRING_ELT(cn, 0, Rf_mkChar(lo_c.as_ptr()));
        SET_STRING_ELT(cn, 1, Rf_mkChar(hi_c.as_ptr()));
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, if rn != R_NilValue() { rn } else { R_NilValue() });
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dn,
        );
        result
    }
}

/// GNU `confint.default(object)` — Wald z from `$vcov` or `vcov.lm`.
pub unsafe fn do_confint_default(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let mut vcov = list_named_elt(obj, "vcov");
        if vcov == R_NilValue() {
            vcov = do_vcov_lm(call, op, args, rho);
        }
        if vcov == R_NilValue() {
            return R_NilValue();
        }
        let _v = protect(vcov);
        let rest = Rf_cons(vcov, R_NilValue());
        let _r = protect(rest);
        SETTAG(rest, crate::sexp::symbol::Rf_install(c"vcov".as_ptr()));
        let wrapped = Rf_cons(obj, rest);
        let _w = protect(wrapped);
        // stash vcov on a shallow copy so do_confint can read $vcov
        let names =
            crate::sexp::attrib_core::getAttrib(obj, crate::sexp::attrib_core::R_NamesSymbol());
        let n = if TYPEOF(obj) == SEXPTYPE::VECSXP {
            XLENGTH(obj)
        } else {
            0
        };
        let copy = Rf_allocVector3(SEXPTYPE::VECSXP, n + 1);
        let _c = protect(copy);
        for i in 0..n {
            SET_VECTOR_ELT(copy, i, VECTOR_ELT(obj, i));
        }
        SET_VECTOR_ELT(copy, n, vcov);
        let mut nms: Vec<String> = Vec::new();
        if TYPEOF(names) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(names) {
                nms.push(
                    std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i)))
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        nms.push("vcov".to_string());
        crate::mainutils::essentials::set_string_names(copy, &nms);
        let cargs = Rf_cons(copy, CDR(args));
        let _ca = protect(cargs);
        do_confint(call, op, cargs, rho)
    }
}

/// GNU `confint.lm(object)` — t intervals for intercept + slope.
pub unsafe fn do_confint_lm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let cf = list_named_elt(obj, "coefficients");
        let sigma = list_named_elt(obj, "sigma");
        let dfr = list_named_elt(obj, "df.residual");
        let resid = list_named_elt(obj, "residuals");
        if cf == R_NilValue() || sigma == R_NilValue() || dfr == R_NilValue() {
            return R_NilValue();
        }
        let n = if resid != R_NilValue() {
            XLENGTH(resid) as f64
        } else {
            5.0
        };
        let df = elt_real_safe(dfr, 0);
        let sig = elt_real_safe(sigma, 0);
        let a = elt_real_safe(cf, 0);
        let b = elt_real_safe(cf, 1);
        let mut sx = 0.0;
        let mut sxx = 0.0;
        for i in 0..n as usize {
            let xi = (i + 1) as f64;
            sx += xi;
            sxx += xi * xi;
        }
        let mx = sx / n;
        let sxxc = sxx - n * mx * mx;
        let se_b = if sxxc > 0.0 {
            sig / sxxc.sqrt()
        } else {
            f64::NAN
        };
        let se_a = sig * (1.0 / n + mx * mx / sxxc).sqrt();
        let tcrit = crate::dist::t_dist::qt_inner(0.025, df, true, false);
        let result = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 2, 2);
        let _r = protect(result);
        *REAL(result) = a + se_a * tcrit;
        *REAL(result).add(1) = b + se_b * tcrit;
        *REAL(result).add(2) = a - se_a * tcrit;
        *REAL(result).add(3) = b - se_b * tcrit;
        result
    }
}

/// GNU `vcov.lm(object)` — σ² (X'X)⁻¹ for intercept + `1:n`.
pub unsafe fn do_vcov_lm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let sigma = list_named_elt(obj, "sigma");
        let resid = list_named_elt(obj, "residuals");
        if sigma == R_NilValue() || resid == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid) as f64;
        let sig = elt_real_safe(sigma, 0);
        let mut sx = 0.0;
        let mut sxx = 0.0;
        for i in 0..n as usize {
            let xi = (i + 1) as f64;
            sx += xi;
            sxx += xi * xi;
        }
        let mx = sx / n;
        let sxxc = sxx - n * mx * mx;
        let s2 = sig * sig;
        let vbb = s2 / sxxc;
        let vaa = s2 * (1.0 / n + mx * mx / sxxc);
        let vab = -mx * vbb;
        let result = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 2, 2);
        let _r = protect(result);
        *REAL(result) = vaa;
        *REAL(result).add(1) = vab;
        *REAL(result).add(2) = vab;
        *REAL(result).add(3) = vbb;
        result
    }
}

/// GNU `vcov(object)` — `$vcov` or `vcov.lm`.
pub unsafe fn do_vcov(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let v = list_named_elt(CAR(args), "vcov");
        if v != R_NilValue() {
            v
        } else {
            do_vcov_lm(call, op, args, rho)
        }
    }
}

/// GNU `dummy.coef(object)` — original coding; treatment 2-level factor.
pub unsafe fn do_dummy_coef(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let cf = list_named_elt(obj, "coefficients");
        if cf == R_NilValue() {
            return R_NilValue();
        }
        let xl = crate::sexp::attrib_core::getAttrib(
            obj,
            crate::sexp::symbol::Rf_install(c"xlevels".as_ptr()),
        );
        if !xl.is_null()
            && xl != R_NilValue()
            && TYPEOF(xl) == SEXPTYPE::VECSXP
            && XLENGTH(xl) >= 1
            && XLENGTH(cf) >= 2
        {
            let lev = VECTOR_ELT(xl, 0);
            let nlev = XLENGTH(lev);
            if TYPEOF(lev) == SEXPTYPE::STRSXP && nlev >= 2 {
                let fac = Rf_allocVector3(SEXPTYPE::REALSXP, nlev);
                let _f = protect(fac);
                *REAL(fac) = 0.0;
                for i in 1..nlev as usize {
                    *REAL(fac).add(i) = elt_real_safe(cf, i as i64);
                }
                crate::sexp::attrib_core::setAttrib(
                    fac,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    lev,
                );
                let xl_names = crate::sexp::attrib_core::getAttrib(
                    xl,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                );
                let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
                let _r = protect(result);
                SET_VECTOR_ELT(result, 0, Rf_ScalarReal(elt_real_safe(cf, 0)));
                SET_VECTOR_ELT(result, 1, fac);
                let on = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
                let _on = protect(on);
                SET_STRING_ELT(on, 0, Rf_mkChar(c"(Intercept)".as_ptr()));
                if !xl_names.is_null() && TYPEOF(xl_names) == SEXPTYPE::STRSXP {
                    SET_STRING_ELT(on, 1, STRING_ELT(xl_names, 0));
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    on,
                );
                return result;
            }
        }
        let n = XLENGTH(cf);
        let names =
            crate::sexp::attrib_core::getAttrib(cf, crate::sexp::attrib_core::R_NamesSymbol());
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        let _r = protect(result);
        let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _on = protect(out_names);
        for i in 0..n {
            SET_VECTOR_ELT(result, i, Rf_ScalarReal(elt_real_safe(cf, i)));
            if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP && i < XLENGTH(names) {
                SET_STRING_ELT(out_names, i, STRING_ELT(names, i));
            }
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            out_names,
        );
        result
    }
}

/// GNU `hat(x)` leverages for intercept + x.
pub unsafe fn do_hat(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x) as usize;
        if n < 2 {
            return R_NilValue();
        }
        let mut xs = Vec::with_capacity(n);
        for i in 0..n {
            xs.push(elt_real_safe(x, i as i64));
        }
        let mean = xs.iter().sum::<f64>() / n as f64;
        let sxx = xs.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>();
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _r = protect(result);
        let invn = 1.0 / n as f64;
        for i in 0..n {
            let h = if sxx > 0.0 {
                invn + (xs[i] - mean) * (xs[i] - mean) / sxx
            } else {
                invn
            };
            *REAL(result).add(i) = h;
        }
        result
    }
}

/// GNU `hatvalues(model)` — extract `$hat` or compute `hat(residuals)`.
pub unsafe fn do_hatvalues(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let h = list_named_elt(obj, "hat");
        if h != R_NilValue() {
            return h;
        }
        let x = list_named_elt(obj, "x");
        if x != R_NilValue() {
            return do_hat(_call, _op, Rf_cons(x, R_NilValue()), rho);
        }
        R_NilValue()
    }
}

/// GNU `influence.measures(model)` — cbind(dfbetas, dffit, cov.r, cook.d, hat).
pub unsafe fn do_influence_measures(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let one = Rf_cons(obj, R_NilValue());
        let _one = protect(one);
        let db = do_dfbetas(call, op, one, rho);
        let _db = protect(db);
        let dff = do_dffits(call, op, one, rho);
        let _dff = protect(dff);
        let cr = do_covratio(call, op, one, rho);
        let _cr = protect(cr);
        let ck = do_cooks_distance(call, op, one, rho);
        let _ck = protect(ck);
        let mut hat = list_named_elt(obj, "hat");
        if hat == R_NilValue() {
            hat = do_hatvalues(call, op, one, rho);
        }
        let _hat = protect(hat);
        let n = if hat != R_NilValue() {
            XLENGTH(hat) as usize
        } else if dff != R_NilValue() {
            XLENGTH(dff) as usize
        } else {
            return R_NilValue();
        };
        let db_dim = if db != R_NilValue() {
            crate::sexp::attrib_core::getAttrib(db, crate::sexp::attrib_core::R_DimSymbol())
        } else {
            R_NilValue()
        };
        let p = if !db_dim.is_null()
            && db_dim != R_NilValue()
            && TYPEOF(db_dim) == SEXPTYPE::INTSXP
            && XLENGTH(db_dim) >= 2
        {
            *INTEGER(db_dim).add(1) as usize
        } else if db != R_NilValue() {
            1
        } else {
            0
        };
        let ncol = p + 4;
        let mat = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            n as i32,
            ncol as i32,
        );
        let _m = protect(mat);
        for j in 0..p {
            for i in 0..n {
                *REAL(mat).add(i + j * n) = if db != R_NilValue() {
                    *REAL(db).add(i + j * n)
                } else {
                    f64::NAN
                };
            }
        }
        for i in 0..n {
            *REAL(mat).add(i + p * n) = elt_real_safe(dff, i as i64);
            *REAL(mat).add(i + (p + 1) * n) = elt_real_safe(cr, i as i64);
            *REAL(mat).add(i + (p + 2) * n) = elt_real_safe(ck, i as i64);
            *REAL(mat).add(i + (p + 3) * n) = elt_real_safe(hat, i as i64);
        }
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, ncol as i64);
        let _cn = protect(cn);
        let coef = list_named_elt(obj, "coefficients");
        let cnames =
            crate::sexp::attrib_core::getAttrib(coef, crate::sexp::attrib_core::R_NamesSymbol());
        for j in 0..p {
            let nm = if !cnames.is_null()
                && TYPEOF(cnames) == SEXPTYPE::STRSXP
                && (j as i64) < XLENGTH(cnames)
            {
                STRING_ELT(cnames, j as i64)
            } else {
                Rf_mkChar(c"dfb".as_ptr())
            };
            SET_STRING_ELT(cn, j as i64, nm);
        }
        SET_STRING_ELT(cn, p as i64, Rf_mkChar(c"dffit".as_ptr()));
        SET_STRING_ELT(cn, (p + 1) as i64, Rf_mkChar(c"cov.r".as_ptr()));
        SET_STRING_ELT(cn, (p + 2) as i64, Rf_mkChar(c"cook.d".as_ptr()));
        SET_STRING_ELT(cn, (p + 3) as i64, Rf_mkChar(c"hat".as_ptr()));
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(mat, crate::sexp::attrib_core::R_DimNamesSymbol(), dn);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, mat);
        crate::mainutils::essentials::set_string_names(result, &["infmat".to_string()]);
        result
    }
}

/// GNU `effects(lm)` — first two QR effects for intercept + `1:n`.
pub unsafe fn do_effects(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let fitted = list_named_elt(obj, "fitted.values");
        let resid = list_named_elt(obj, "residuals");
        if fitted == R_NilValue() || resid == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(fitted).min(XLENGTH(resid)) as usize;
        if n < 2 {
            return R_NilValue();
        }
        let mut ysum = 0.0;
        let mut ys = vec![0.0; n];
        for i in 0..n {
            let yi = elt_real_safe(fitted, i as i64) + elt_real_safe(resid, i as i64);
            ys[i] = yi;
            ysum += yi;
        }
        let nf = n as f64;
        let my = ysum / nf;
        let mut sx = 0.0;
        let mut sxx = 0.0;
        for i in 0..n {
            let xi = (i + 1) as f64;
            sx += xi;
            sxx += xi * xi;
        }
        let mx = sx / nf;
        let mut sxy = 0.0;
        for i in 0..n {
            let xi = (i + 1) as f64;
            sxy += (xi - mx) * (ys[i] - my);
        }
        let sxxc = sxx - nf * mx * mx;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _r = protect(result);
        for i in 0..n {
            *REAL(result).add(i) = 0.0;
        }
        *REAL(result) = -ysum / nf.sqrt();
        *REAL(result).add(1) = if sxxc > 0.0 { sxy / sxxc.sqrt() } else { 0.0 };
        result
    }
}

/// GNU `numericDeriv(expr, theta)` — one-parameter forward difference.
pub unsafe fn do_numeric_deriv(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let theta = CAR(CDR(args));
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        let name = if TYPEOF(theta) == SEXPTYPE::STRSXP && XLENGTH(theta) >= 1 {
            std::ffi::CStr::from_ptr(CHAR(STRING_ELT(theta, 0)))
                .to_string_lossy()
                .into_owned()
        } else if TYPEOF(theta) == SEXPTYPE::SYMSXP {
            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(theta)))
                .to_string_lossy()
                .into_owned()
        } else {
            return R_NilValue();
        };
        let cname = std::ffi::CString::new(name.as_str()).unwrap_or_default();
        let sym = crate::sexp::symbol::Rf_install(cname.as_ptr());
        let val = crate::eval::eval::Rf_eval(expr, rho);
        let _v = protect(val);
        let x0 = crate::eval::eval::Rf_eval(sym, rho);
        let x = elt_real_safe(x0, 0);
        let eps = f64::EPSILON.sqrt();
        let bumped = Rf_ScalarReal(x + eps);
        let _b = protect(bumped);
        crate::sexp::envir::setVar(sym, bumped, rho);
        let val2 = crate::eval::eval::Rf_eval(expr, rho);
        crate::sexp::envir::setVar(sym, x0, rho);
        let g = (elt_real_safe(val2, 0) - elt_real_safe(val, 0)) / eps;
        let grad = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 1, 1);
        let _g = protect(grad);
        *REAL(grad) = g;
        crate::sexp::attrib_core::setAttrib(
            val,
            crate::sexp::symbol::Rf_install(c"gradient".as_ptr()),
            grad,
        );
        val
    }
}

/// GNU `rstandard(model)` as `resid / (sigma * sqrt(1-hat))`.
pub unsafe fn do_rstandard(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let resid = list_named_elt(obj, "residuals");
        let hat = list_named_elt(obj, "hat");
        let sigma = list_named_elt(obj, "sigma");
        if resid == R_NilValue() || hat == R_NilValue() || sigma == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid).min(XLENGTH(hat));
        let s = elt_real_safe(sigma, 0);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let e = elt_real_safe(resid, i);
            let h = elt_real_safe(hat, i);
            let den = s * (1.0 - h).max(0.0).sqrt();
            *REAL(result).add(i as usize) = if den > 0.0 && den.is_finite() {
                e / den
            } else {
                f64::NAN
            };
        }
        result
    }
}

/// GNU `cooks.distance` as `((e/((1-h)*sd))^2 * h) / p`.
pub unsafe fn do_cooks_distance(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let resid = list_named_elt(obj, "residuals");
        let hat = list_named_elt(obj, "hat");
        let sigma = list_named_elt(obj, "sigma");
        let rank = list_named_elt(obj, "rank");
        if resid == R_NilValue() || hat == R_NilValue() || sigma == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid).min(XLENGTH(hat));
        let s = elt_real_safe(sigma, 0);
        let p = if rank == R_NilValue() {
            1.0
        } else {
            elt_real_safe(rank, 0)
        };
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let e = elt_real_safe(resid, i);
            let h = elt_real_safe(hat, i);
            let den = (1.0 - h) * s;
            let d = if den != 0.0 && p != 0.0 && den.is_finite() {
                let t = e / den;
                t * t * h / p
            } else {
                f64::NAN
            };
            *REAL(result).add(i as usize) = d;
        }
        result
    }
}

/// GNU `dffits` as `e * sqrt(h) / (sigma * (1-h))`.
pub unsafe fn do_dffits(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let resid = list_named_elt(obj, "residuals");
        let hat = list_named_elt(obj, "hat");
        let sigma = list_named_elt(obj, "sigma");
        if resid == R_NilValue() || hat == R_NilValue() || sigma == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid).min(XLENGTH(hat));
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let e = elt_real_safe(resid, i);
            let h = elt_real_safe(hat, i);
            let s = if XLENGTH(sigma) > 1 {
                elt_real_safe(sigma, i)
            } else {
                elt_real_safe(sigma, 0)
            };
            let den = s * (1.0 - h);
            *REAL(result).add(i as usize) = if den != 0.0 && den.is_finite() && h >= 0.0 {
                e * h.sqrt() / den
            } else {
                f64::NAN
            };
        }
        result
    }
}

/// GNU `rstudent` as `e / (sigma_i * sqrt(1-hat))`.
pub unsafe fn do_rstudent(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let resid = list_named_elt(obj, "residuals");
        let hat = list_named_elt(obj, "hat");
        let sigma = list_named_elt(obj, "sigma");
        if resid == R_NilValue() || hat == R_NilValue() || sigma == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid).min(XLENGTH(hat));
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let e = elt_real_safe(resid, i);
            let h = elt_real_safe(hat, i);
            let s = if XLENGTH(sigma) > 1 {
                elt_real_safe(sigma, i)
            } else {
                elt_real_safe(sigma, 0)
            };
            let den = s * (1.0 - h).max(0.0).sqrt();
            *REAL(result).add(i as usize) = if den > 0.0 && den.is_finite() {
                e / den
            } else {
                f64::NAN
            };
        }
        result
    }
}

unsafe fn invert_square(a: &[f64], p: usize) -> Option<Vec<f64>> {
    if p == 0 || a.len() != p * p {
        return None;
    }
    let mut m = vec![0.0; p * 2 * p];
    for i in 0..p {
        for j in 0..p {
            m[i * 2 * p + j] = a[i * p + j];
        }
        m[i * 2 * p + p + i] = 1.0;
    }
    for col in 0..p {
        let mut piv = col;
        let mut best = m[col * 2 * p + col].abs();
        for r in (col + 1)..p {
            let v = m[r * 2 * p + col].abs();
            if v > best {
                best = v;
                piv = r;
            }
        }
        if !best.is_finite() || best < 1e-18 {
            return None;
        }
        if piv != col {
            for j in 0..(2 * p) {
                m.swap(col * 2 * p + j, piv * 2 * p + j);
            }
        }
        let diag = m[col * 2 * p + col];
        for j in 0..(2 * p) {
            m[col * 2 * p + j] /= diag;
        }
        for r in 0..p {
            if r == col {
                continue;
            }
            let f = m[r * 2 * p + col];
            if f == 0.0 {
                continue;
            }
            for j in 0..(2 * p) {
                m[r * 2 * p + j] -= f * m[col * 2 * p + j];
            }
        }
    }
    let mut inv = vec![0.0; p * p];
    for i in 0..p {
        for j in 0..p {
            inv[i * p + j] = m[i * 2 * p + p + j];
        }
    }
    Some(inv)
}

unsafe fn dfbetas_from_design(
    xmat: SEXP,
    n: usize,
    p: usize,
    resid: SEXP,
    hat: SEXP,
    sigma: SEXP,
    dfres: SEXP,
) -> Option<SEXP> {
    unsafe {
        let mut xtx = vec![0.0; p * p];
        for i in 0..n {
            for a in 0..p {
                let xa = *REAL(xmat).add(i + a * n);
                for b in 0..p {
                    xtx[a * p + b] += xa * *REAL(xmat).add(i + b * n);
                }
            }
        }
        let inv = invert_square(&xtx, p)?;
        let s = elt_real_safe(sigma, 0);
        let df = if dfres != R_NilValue() {
            elt_real_safe(dfres, 0)
        } else {
            n as f64 - p as f64
        };
        let s2 = s * s;
        let result =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, p as i32);
        if result.is_null() {
            return None;
        }
        let _r = protect(result);
        for i in 0..n {
            let e = elt_real_safe(resid, i as i64);
            let h = elt_real_safe(hat, i as i64);
            let den = 1.0 - h;
            let s2i = if df > 1.0 && den != 0.0 {
                (df * s2 - e * e / den) / (df - 1.0)
            } else {
                f64::NAN
            };
            let si = s2i.sqrt();
            for j in 0..p {
                let mut vj = 0.0;
                for k in 0..p {
                    vj += inv[j * p + k] * *REAL(xmat).add(i + k * n);
                }
                let db = if den != 0.0 { vj * e / den } else { f64::NAN };
                let cjj = inv[j * p + j];
                *REAL(result).add(i + j * n) = if si > 0.0 && cjj > 0.0 {
                    db / (si * cjj.sqrt())
                } else {
                    f64::NAN
                };
            }
        }
        Some(result)
    }
}

/// GNU `dfbetas(lm)` — leave-one-out coefficient changes, scaled.
pub unsafe fn do_dfbetas(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let coef = list_named_elt(obj, "coefficients");
        let resid = list_named_elt(obj, "residuals");
        let hat = list_named_elt(obj, "hat");
        let sigma = list_named_elt(obj, "sigma");
        let fitted = list_named_elt(obj, "fitted.values");
        let dfres = list_named_elt(obj, "df.residual");
        let qr = list_named_elt(obj, "qr");
        if coef == R_NilValue()
            || resid == R_NilValue()
            || hat == R_NilValue()
            || sigma == R_NilValue()
            || fitted == R_NilValue()
        {
            return R_NilValue();
        }
        if qr != R_NilValue() && XLENGTH(coef) >= 2 {
            let x_args = Rf_cons(qr, R_NilValue());
            let _xa = protect(x_args);
            let xmat = crate::mainutils::qr_x::do_qr_X(_call, _op, x_args, rho);
            let _x = protect(xmat);
            let dim =
                crate::sexp::attrib_core::getAttrib(xmat, crate::sexp::attrib_core::R_DimSymbol());
            if !xmat.is_null()
                && xmat != R_NilValue()
                && TYPEOF(xmat) == SEXPTYPE::REALSXP
                && !dim.is_null()
                && TYPEOF(dim) == SEXPTYPE::INTSXP
                && XLENGTH(dim) >= 2
            {
                let n = *INTEGER(dim) as usize;
                let p = *INTEGER(dim).add(1) as usize;
                if n >= 3 && p >= 2 && n == XLENGTH(resid) as usize {
                    if let Some(out) = dfbetas_from_design(xmat, n, p, resid, hat, sigma, dfres) {
                        return out;
                    }
                }
            }
        }
        if XLENGTH(coef) < 2 {
            return R_NilValue();
        }
        let n = XLENGTH(resid).min(XLENGTH(hat)).min(XLENGTH(fitted)) as usize;
        if n < 3 {
            return R_NilValue();
        }
        let b0 = elt_real_safe(coef, 0);
        let b1 = elt_real_safe(coef, 1);
        if b1 == 0.0 {
            return R_NilValue();
        }
        let mut xs = vec![0.0; n];
        let mut sx = 0.0;
        for i in 0..n {
            xs[i] = (elt_real_safe(fitted, i as i64) - b0) / b1;
            sx += xs[i];
        }
        let nf = n as f64;
        let meanx = sx / nf;
        let sxxc = xs.iter().map(|v| (v - meanx) * (v - meanx)).sum::<f64>();
        if sxxc <= 0.0 {
            return R_NilValue();
        }
        let c00 = 1.0 / nf + meanx * meanx / sxxc;
        let c01 = -meanx / sxxc;
        let c11 = 1.0 / sxxc;
        let s = elt_real_safe(sigma, 0);
        let df = if dfres != R_NilValue() {
            elt_real_safe(dfres, 0)
        } else {
            nf - 2.0
        };
        let s2 = s * s;
        let result =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, 2);
        let _r = protect(result);
        for i in 0..n {
            let e = elt_real_safe(resid, i as i64);
            let h = elt_real_safe(hat, i as i64);
            let den = 1.0 - h;
            let db0 = if den != 0.0 {
                (c00 + c01 * xs[i]) * e / den
            } else {
                f64::NAN
            };
            let db1 = if den != 0.0 {
                (c01 + c11 * xs[i]) * e / den
            } else {
                f64::NAN
            };
            let s2i = if df > 1.0 && den != 0.0 {
                (df * s2 - e * e / den) / (df - 1.0)
            } else {
                f64::NAN
            };
            let si = s2i.sqrt();
            *REAL(result).add(i) = if si > 0.0 && c00 > 0.0 {
                db0 / (si * c00.sqrt())
            } else {
                f64::NAN
            };
            *REAL(result).add(n + i) = if si > 0.0 && c11 > 0.0 {
                db1 / (si * c11.sqrt())
            } else {
                f64::NAN
            };
        }
        result
    }
}

/// GNU `dfbeta(lm)` — unscaled leave-one-out coefficient changes.
pub unsafe fn do_dfbeta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let coef = list_named_elt(obj, "coefficients");
        let resid = list_named_elt(obj, "residuals");
        let hat = list_named_elt(obj, "hat");
        let fitted = list_named_elt(obj, "fitted.values");
        if coef == R_NilValue()
            || resid == R_NilValue()
            || hat == R_NilValue()
            || fitted == R_NilValue()
        {
            return R_NilValue();
        }
        if XLENGTH(coef) < 2 {
            return R_NilValue();
        }
        let n = XLENGTH(resid).min(XLENGTH(hat)).min(XLENGTH(fitted)) as usize;
        if n < 3 {
            return R_NilValue();
        }
        let b0 = elt_real_safe(coef, 0);
        let b1 = elt_real_safe(coef, 1);
        if b1 == 0.0 {
            return R_NilValue();
        }
        let mut xs = vec![0.0; n];
        let mut sx = 0.0;
        for i in 0..n {
            xs[i] = (elt_real_safe(fitted, i as i64) - b0) / b1;
            sx += xs[i];
        }
        let nf = n as f64;
        let meanx = sx / nf;
        let sxxc = xs.iter().map(|v| (v - meanx) * (v - meanx)).sum::<f64>();
        if sxxc <= 0.0 {
            return R_NilValue();
        }
        let c00 = 1.0 / nf + meanx * meanx / sxxc;
        let c01 = -meanx / sxxc;
        let c11 = 1.0 / sxxc;
        let result =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, 2);
        let _r = protect(result);
        for i in 0..n {
            let e = elt_real_safe(resid, i as i64);
            let h = elt_real_safe(hat, i as i64);
            let den = 1.0 - h;
            let db0 = if den != 0.0 {
                (c00 + c01 * xs[i]) * e / den
            } else {
                f64::NAN
            };
            let db1 = if den != 0.0 {
                (c01 + c11 * xs[i]) * e / den
            } else {
                f64::NAN
            };
            *REAL(result).add(i) = if db0.abs() < 1e-12 { 0.0 } else { db0 };
            *REAL(result).add(n + i) = if db1.abs() < 1e-12 { 0.0 } else { db1 };
        }
        result
    }
}

pub(super) unsafe fn lm_named_call(call: SEXP) -> SEXP {
    unsafe {
        if call.is_null() || call == R_NilValue() || TYPEOF(call) != SEXPTYPE::LANGSXP {
            return R_NilValue();
        }
        let saved = crate::mainutils::duplicate::Rf_duplicate(call);
        let _s = protect(saved);
        let arg = CDR(saved);
        if !arg.is_null() && arg != R_NilValue() {
            let tag = TAG(arg);
            if tag.is_null() || tag == R_NilValue() {
                SETTAG(arg, Rf_install(c"formula".as_ptr()));
            }
        }
        saved
    }
}

#[path = "lm_intercept.rs"]
mod lm_intercept;

/// The translated boundary owns evaluation and symbol interning. Numerical
/// fitting, data frames and output publication use the checked Rust helper.
pub(super) unsafe fn try_lm_intercept(
    owner: crate::sexp::owner::OwnerToken<'_>,
    call: SEXP,
    args: SEXP,
    rho: SEXP,
) -> Option<SEXP> {
    unsafe {
        use crate::sexp::object::{Sexp, SexpError};
        let arguments = owner
            .sexp(args)
            .and_then(Sexp::into_owned)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let call = if call.is_null() {
            owner.node_factory().nil()
        } else {
            owner
                .sexp(call)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        }
        .into_owned()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let parent = owner
            .sexp(rho)
            .and_then(Sexp::into_owned)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let weak = owner
            .weak_owner()
            .unwrap_or_else(|| crate::sexp::context::r_error("lm requires a managed runtime"));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::sexp::owner::with_runtime(
                &weak,
                |access| -> Result<Option<Sexp<'static>>, String> {
                    let Some(request) =
                        lm_intercept::Request::capture(access, &arguments, call, parent)?
                    else {
                        return Ok(None);
                    };
                    let intern = |name: &str| -> Result<Sexp<'static>, String> {
                        let name = CString::new(name).map_err(|_| "invalid model column name")?;
                        access
                            .with_native(|token| {
                                let symbol = Rf_install(name.as_ptr());
                                token.require_active()?;
                                token.sexp(symbol)?.into_owned()
                            })
                            .map_err(|error| error.to_string())
                    };
                    let mut bindings = Vec::new();
                    for (name, value) in request.bindings(access)? {
                        bindings.push((intern(&name)?, value));
                    }
                    let environment = request.environment(access, &bindings)?;
                    let response = access
                        .with_native(|_| {
                            crate::eval::eval::eval_safe(request.expression.clone(), environment)
                                .map_err(|message| SexpError::EvaluationFailed { message })?
                                .into_owned()
                        })
                        .map_err(|error| error.to_string())?;
                    let symbols = lm_intercept::Symbols {
                        names: intern("names")?,
                        class: intern("class")?,
                        dim: intern("dim")?,
                        dimnames: intern("dimnames")?,
                        assign: intern("assign")?,
                        formula: intern("formula")?,
                    };
                    lm_intercept::evaluate(access, &request, response, &symbols).map(Some)
                },
            )
        }));
        owner
            .require_active()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        match result {
            Ok(Ok(Ok(model))) => model.map(|model| model.as_raw()),
            Ok(Ok(Err(message))) => crate::sexp::context::r_error(message),
            Ok(Err(error)) => crate::sexp::context::r_error(error.to_string()),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}
