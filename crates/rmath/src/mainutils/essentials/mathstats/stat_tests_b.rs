//! More tests (mcnemar, fisher, friedman, mantelhaen, pairwise, pp) and factor rotations — extracted verbatim from essentials/mathstats.rs.

use super::*;

/// GNU 2x2 `mcnemar.test(x, correct=TRUE)`.
pub unsafe fn do_mcnemar_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || XLENGTH(x) < 4 {
            return R_NilValue();
        }
        let mut correct = true;
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let c = CAR(rest);
            if TYPEOF(c) == SEXPTYPE::LGLSXP && XLENGTH(c) > 0 {
                correct = *INTEGER(c) != 0;
            }
        }
        let a12 = elt_real_safe(x, 2);
        let a21 = elt_real_safe(x, 1);
        let off = a12 + a21;
        let y = if correct && (a12 - a21).abs() > 0.0 {
            (a12 - a21).abs() - 1.0
        } else {
            a12 - a21
        };
        let stat = if off > 0.0 { y * y / off } else { 0.0 };
        let pval = crate::dist::chisq::pchisq_inner(stat, 1.0, false, false);
        let method = if correct && (a12 - a21).abs() > 0.0 {
            "McNemar's Chi-squared test with continuity correction"
        } else {
            "McNemar's Chi-squared test"
        };
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["McNemar's chi-squared".to_string()]);
        let parameter = Rf_ScalarReal(1.0);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        let m = CString::new(method).unwrap_or_default();
        SET_VECTOR_ELT(result, 3, Rf_mkString(m.as_ptr()));
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x".as_ptr()));
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

/// GNU 2x2 `fisher.test(x)`.
pub unsafe fn do_fisher_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let tab = CAR(args);
        if tab.is_null() || tab == R_NilValue() || XLENGTH(tab) < 4 {
            return R_NilValue();
        }

        let a11 = elt_real_safe(tab, 0);
        let a21 = elt_real_safe(tab, 1);
        let a12 = elt_real_safe(tab, 2);
        let a22 = elt_real_safe(tab, 3);
        let m = a11 + a21;
        let n = a12 + a22;
        let k = a11 + a12;
        let x = a11;
        let lo = 0.0_f64.max(k - n);
        let hi = k.min(m);
        let dobs = crate::dist::hypergeometric::dhyper_inner(x, m, n, k, false);
        let mut pval = 0.0;
        let mut t = lo;
        while t <= hi + 0.5 {
            let dt = crate::dist::hypergeometric::dhyper_inner(t, m, n, k, false);
            if dt <= dobs * (1.0 + 1e-7) {
                pval += dt;
            }
            t += 1.0;
        }
        pval = pval.min(1.0);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(pval));
        let nv = Rf_ScalarReal(1.0);
        let _nv = protect(nv);
        set_string_names(nv, &["odds ratio".to_string()]);
        SET_VECTOR_ELT(result, 1, nv);
        SET_VECTOR_ELT(result, 2, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(
            result,
            3,
            Rf_mkString(c"Fisher's Exact Test for Count Data".as_ptr()),
        );
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "p.value".to_string(),
                "null.value".to_string(),
                "alternative".to_string(),
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

/// GNU unreplicated-block `friedman.test(x)`.
pub unsafe fn do_friedman_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (n, k) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            return R_NilValue();
        };
        if n < 2 || k < 2 {
            return R_NilValue();
        }
        let mut ranks = vec![0.0; n * k];
        let mut tie_sum = 0.0;
        for i in 0..n {
            let mut row = Vec::with_capacity(k);
            for j in 0..k {
                row.push(elt_real_safe(x, (j * n + i) as i64));
            }
            let rr = rank_average(&row);
            let mut counts: Vec<(f64, i32)> = Vec::new();
            for (j, &r) in rr.iter().enumerate() {
                ranks[j * n + i] = r;
                if let Some(c) = counts.iter_mut().find(|(v, _)| *v == r) {
                    c.1 += 1;
                } else {
                    counts.push((r, 1));
                }
            }
            for (_, u) in counts {
                let uf = u as f64;
                tie_sum += uf * uf * uf - uf;
            }
        }
        let expect = n as f64 * (k as f64 + 1.0) / 2.0;
        let mut ss = 0.0;
        for j in 0..k {
            let mut s = 0.0;
            for i in 0..n {
                s += ranks[j * n + i];
            }
            let d = s - expect;
            ss += d * d;
        }
        let denom = (n * k * (k + 1)) as f64 - tie_sum / (k as f64 - 1.0);
        let stat = if denom > 0.0 { 12.0 * ss / denom } else { 0.0 };
        let df = (k - 1) as f64;
        let pval = crate::dist::chisq::pchisq_inner(stat, df, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Friedman chi-squared".to_string()]);
        let parameter = Rf_ScalarReal(df);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, Rf_mkString(c"Friedman rank sum test".as_ptr()));
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x".as_ptr()));
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

/// GNU unreplicated-block `quade.test(x)`.
pub unsafe fn do_quade_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (b, k) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            return R_NilValue();
        };
        if b < 2 || k < 2 {
            return R_NilValue();
        }
        let mid = (k as f64 + 1.0) / 2.0;
        let mut ranges = vec![0.0; b];
        let mut s = vec![0.0; b * k];
        for i in 0..b {
            let mut row = Vec::with_capacity(k);
            for j in 0..k {
                row.push(elt_real_safe(x, (j * b + i) as i64));
            }
            let rr = rank_average(&row);
            let mut mn = f64::INFINITY;
            let mut mx = f64::NEG_INFINITY;
            for &v in &row {
                mn = mn.min(v);
                mx = mx.max(v);
            }
            ranges[i] = mx - mn;
            for j in 0..k {
                s[j * b + i] = rr[j] - mid;
            }
        }
        let q = rank_average(&ranges);
        let mut a = 0.0;
        let mut col = vec![0.0; k];
        for i in 0..b {
            for j in 0..k {
                let v = q[i] * s[j * b + i];
                s[j * b + i] = v;
                a += v * v;
                col[j] += v;
            }
        }
        let mut bss = 0.0;
        for c in col {
            bss += c * c;
        }
        let bb = bss / (b as f64);
        let stat = if (a - bb).abs() < 1e-15 {
            f64::NAN
        } else {
            (b as f64 - 1.0) * bb / (a - bb)
        };
        let df1 = (k - 1) as f64;
        let df2 = ((b - 1) * (k - 1)) as f64;
        let pval = crate::dist::f_dist::pf_inner(stat, df1, df2, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Quade F".to_string()]);
        let parameter = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _pa = protect(parameter);
        *REAL(parameter) = df1;
        *REAL(parameter).add(1) = df2;
        set_string_names(parameter, &["num df".to_string(), "denom df".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, Rf_mkString(c"Quade test".as_ptr()));
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x".as_ptr()));
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

/// GNU 2x2xK `mantelhaen.test(x)` (asymptotic, two-sided).
pub unsafe fn do_mantelhaen_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null()
            || dim == R_NilValue()
            || TYPEOF(dim) != SEXPTYPE::INTSXP
            || XLENGTH(dim) < 3
        {
            return R_NilValue();
        }
        let i = *INTEGER(dim) as usize;
        let j = *INTEGER(dim).add(1) as usize;
        let k = *INTEGER(dim).add(2) as usize;
        if i != 2 || j != 2 || k < 1 {
            return R_NilValue();
        }
        let mut delta = 0.0;
        let mut varsum = 0.0;
        let mut s_diag = 0.0;
        let mut s_offd = 0.0;
        for s in 0..k {
            let a = elt_real_safe(x, (s * 4) as i64);
            let b = elt_real_safe(x, (s * 4 + 1) as i64);
            let c = elt_real_safe(x, (s * 4 + 2) as i64);
            let d = elt_real_safe(x, (s * 4 + 3) as i64);
            let n = a + b + c + d;
            if n <= 1.0 {
                continue;
            }
            let sx1 = a + c;
            let sx2 = b + d;
            let sy1 = a + b;
            let sy2 = c + d;
            delta += a - sx1 * sy1 / n;
            varsum += sx1 * sx2 * sy1 * sy2 / (n * n * (n - 1.0));
            s_diag += a * d / n;
            s_offd += c * b / n;
        }
        let yates = if delta.abs() >= 0.5 { 0.5 } else { 0.0 };
        let stat = if varsum > 0.0 {
            let num = delta.abs() - yates;
            num * num / varsum
        } else {
            0.0
        };
        let pval = crate::dist::chisq::pchisq_inner(stat, 1.0, false, false);
        let estimate = if s_offd > 0.0 {
            s_diag / s_offd
        } else {
            f64::INFINITY
        };
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Mantel-Haenszel X-squared".to_string()]);
        let parameter = Rf_ScalarReal(1.0);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let est = Rf_ScalarReal(estimate);
        let _es = protect(est);
        set_string_names(est, &["common odds ratio".to_string()]);
        let method_s = if yates > 0.0 {
            Rf_mkString(c"Mantel-Haenszel chi-squared test with continuity correction".as_ptr())
        } else {
            Rf_mkString(c"Mantel-Haenszel chi-squared test without continuity correction".as_ptr())
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 6);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, est);
        SET_VECTOR_ELT(result, 4, method_s);
        SET_VECTOR_ELT(result, 5, Rf_mkString(c"x".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "estimate".to_string(),
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

/// GNU `pairwise.t.test(x, g)` with pooled SD.
pub unsafe fn do_pairwise_t_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let g = CAR(CDR(args));
        let mut method = "holm".to_string();
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
            if name == "p.adjust.method" || name.is_empty() {
                let v = CAR(cell);
                if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                    method = elt_to_string(v, 0);
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
        let mut means = vec![0.0; k];
        let mut vars = vec![0.0; k];
        let mut ns = vec![0.0; k];
        for (li, &lev) in levels.iter().enumerate() {
            let vs: Vec<f64> = pairs.iter().filter(|p| p.0 == lev).map(|p| p.1).collect();
            let n = vs.len() as f64;
            ns[li] = n;
            let m = vs.iter().sum::<f64>() / n;
            means[li] = m;
            vars[li] = vs.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (n - 1.0);
        }
        let total_df: f64 = ns.iter().map(|n| n - 1.0).sum();
        let pooled = (ns
            .iter()
            .zip(vars.iter())
            .map(|(n, v)| v * (n - 1.0))
            .sum::<f64>()
            / total_df)
            .sqrt();
        let mut raw = Vec::new();
        for i in 1..k {
            for j in 0..i {
                let se = pooled * (1.0 / ns[i] + 1.0 / ns[j]).sqrt();
                let t = (means[i] - means[j]) / se;
                let p = 2.0 * crate::dist::t_dist::pt_inner(-t.abs(), total_df, true, false);
                raw.push(p);
            }
        }
        let adj = p_adjust_values(&raw, &method);
        // lower-tri including diag of (k-1) x (k-1), column-major
        let mdim = k - 1;
        let pmat = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            mdim as i32,
            mdim as i32,
        );
        let _pm = protect(pmat);
        for idx in 0..(mdim * mdim) {
            *REAL(pmat).add(idx) = NA_REAL;
        }
        let mut t = 0usize;
        for j in 0..mdim {
            for i in 0..mdim {
                if i >= j {
                    *REAL(pmat).add(i + j * mdim) = adj[t];
                    t += 1;
                }
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_mkString(c"t tests with pooled SD".as_ptr()));
        SET_VECTOR_ELT(result, 1, pmat);
        let meth = if method == "none" {
            Rf_mkString(c"none".as_ptr())
        } else {
            Rf_mkString(c"holm".as_ptr())
        };
        SET_VECTOR_ELT(result, 2, meth);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "method".to_string(),
                "p.value".to_string(),
                "p.adjust.method".to_string(),
            ],
        );
        let class = Rf_mkString(c"pairwise.htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn prop_test_two(x1: f64, n1: f64, x2: f64, n2: f64) -> f64 {
    let p = (x1 + x2) / (n1 + n2);
    let e11 = n1 * p;
    let e12 = n1 * (1.0 - p);
    let e21 = n2 * p;
    let e22 = n2 * (1.0 - p);
    let delta = (x1 / n1 - x2 / n2).abs();
    let yates = 0.5_f64.min(delta / (1.0 / n1 + 1.0 / n2));
    let stat = (x1 - e11).abs() - yates;
    let stat = stat * stat / e11
        + ((n1 - x1 - e12).abs() - yates).powi(2) / e12
        + ((x2 - e21).abs() - yates).powi(2) / e21
        + ((n2 - x2 - e22).abs() - yates).powi(2) / e22;
    crate::dist::chisq::pchisq_inner(stat, 1.0, false, false)
}

/// GNU `pairwise.prop.test(x, n)` with Holm.
pub unsafe fn do_pairwise_prop_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = CAR(CDR(args));
        let mut method = "holm".to_string();
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
            if name == "p.adjust.method" || name.is_empty() {
                let v = CAR(cell);
                if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                    method = elt_to_string(v, 0);
                }
            }
            cell = CDR(cell);
        }
        if x.is_null() || x == R_NilValue() || n.is_null() || n == R_NilValue() {
            return R_NilValue();
        }
        let k = XLENGTH(x).min(XLENGTH(n)) as usize;
        if k < 2 {
            return R_NilValue();
        }
        let mut xs = Vec::with_capacity(k);
        let mut ns = Vec::with_capacity(k);
        for i in 0..k {
            xs.push(elt_real_safe(x, i as i64));
            ns.push(elt_real_safe(n, i as i64));
        }
        let mut raw = Vec::new();
        for i in 1..k {
            for j in 0..i {
                raw.push(prop_test_two(xs[j], ns[j], xs[i], ns[i]));
            }
        }
        let adj = p_adjust_values(&raw, &method);
        let mdim = k - 1;
        let pmat = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            mdim as i32,
            mdim as i32,
        );
        let _pm = protect(pmat);
        for idx in 0..(mdim * mdim) {
            *REAL(pmat).add(idx) = NA_REAL;
        }
        let mut t = 0usize;
        for j in 0..mdim {
            for i in 0..mdim {
                if i >= j {
                    *REAL(pmat).add(i + j * mdim) = adj[t];
                    t += 1;
                }
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(
            result,
            0,
            Rf_mkString(c"Pairwise comparison of proportions".as_ptr()),
        );
        SET_VECTOR_ELT(result, 1, pmat);
        let meth = if method == "none" {
            Rf_mkString(c"none".as_ptr())
        } else {
            Rf_mkString(c"holm".as_ptr())
        };
        SET_VECTOR_ELT(result, 2, meth);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "method".to_string(),
                "p.value".to_string(),
                "p.adjust.method".to_string(),
            ],
        );
        let class = Rf_mkString(c"pairwise.htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn wilcox_two_asymp(x: &[f64], y: &[f64]) -> f64 {
    let nx = x.len() as f64;
    let ny = y.len() as f64;
    if nx < 1.0 || ny < 1.0 {
        return f64::NAN;
    }
    let mut vals: Vec<(f64, u8)> = x.iter().map(|v| (*v, 0u8)).collect();
    vals.extend(y.iter().map(|v| (*v, 1u8)));
    vals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let n = vals.len();
    let mut ranks = vec![0.0; n];
    let mut i = 0;
    let mut nties3 = 0.0;
    while i < n {
        let mut j = i + 1;
        while j < n && vals[j].0 == vals[i].0 {
            j += 1;
        }
        let r = (i + 1 + j) as f64 / 2.0;
        let u = (j - i) as f64;
        nties3 += u * u * u - u;
        for k in i..j {
            ranks[k] = r;
        }
        i = j;
    }
    let mut wx = 0.0;
    for (k, (_, which)) in vals.iter().enumerate() {
        if *which == 0 {
            wx += ranks[k];
        }
    }
    let w = wx - nx * (nx + 1.0) / 2.0;
    let nxy = nx + ny;
    let sigma = ((nx * ny / 12.0) * ((nxy + 1.0) - nties3 / (nxy * (nxy - 1.0)))).sqrt();
    let mut z = w - nx * ny / 2.0;
    if z != 0.0 {
        z -= z.signum() * 0.5;
    }
    z /= sigma;
    let p = crate::dist::normal::pnorm5_inner(z, 0.0, 1.0, true, false);
    (2.0 * p.min(1.0 - p)).min(1.0)
}

/// GNU `pairwise.wilcox.test(x, g)` asymptotic with continuity.
pub unsafe fn do_pairwise_wilcox_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let g = CAR(CDR(args));
        let mut method = "holm".to_string();
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
            if name == "p.adjust.method" || name.is_empty() {
                let v = CAR(cell);
                if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                    method = elt_to_string(v, 0);
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
        let groups: Vec<Vec<f64>> = levels
            .iter()
            .map(|lev| pairs.iter().filter(|p| p.0 == *lev).map(|p| p.1).collect())
            .collect();
        let mut raw = Vec::new();
        for i in 1..k {
            for j in 0..i {
                raw.push(wilcox_two_asymp(&groups[j], &groups[i]));
            }
        }
        let adj = p_adjust_values(&raw, &method);
        let mdim = k - 1;
        let pmat = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            mdim as i32,
            mdim as i32,
        );
        let _pm = protect(pmat);
        for idx in 0..(mdim * mdim) {
            *REAL(pmat).add(idx) = NA_REAL;
        }
        let mut t = 0usize;
        for j in 0..mdim {
            for i in 0..mdim {
                if i >= j {
                    *REAL(pmat).add(i + j * mdim) = adj[t];
                    t += 1;
                }
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(
            result,
            0,
            Rf_mkString(c"Wilcoxon rank sum test with continuity correction".as_ptr()),
        );
        SET_VECTOR_ELT(result, 1, pmat);
        let meth = if method == "none" {
            Rf_mkString(c"none".as_ptr())
        } else {
            Rf_mkString(c"holm".as_ptr())
        };
        SET_VECTOR_ELT(result, 2, meth);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "method".to_string(),
                "p.value".to_string(),
                "p.adjust.method".to_string(),
            ],
        );
        let class = Rf_mkString(c"pairwise.htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn invert3(a: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    if !det.is_finite() || det.abs() < 1e-14 {
        return None;
    }
    let mut inv = [[0.0; 3]; 3];
    inv[0][0] = (a[1][1] * a[2][2] - a[1][2] * a[2][1]) / det;
    inv[0][1] = (a[0][2] * a[2][1] - a[0][1] * a[2][2]) / det;
    inv[0][2] = (a[0][1] * a[1][2] - a[0][2] * a[1][1]) / det;
    inv[1][0] = (a[1][2] * a[2][0] - a[1][0] * a[2][2]) / det;
    inv[1][1] = (a[0][0] * a[2][2] - a[0][2] * a[2][0]) / det;
    inv[1][2] = (a[0][2] * a[1][0] - a[0][0] * a[1][2]) / det;
    inv[2][0] = (a[1][0] * a[2][1] - a[1][1] * a[2][0]) / det;
    inv[2][1] = (a[0][1] * a[2][0] - a[0][0] * a[2][1]) / det;
    inv[2][2] = (a[0][0] * a[1][1] - a[0][1] * a[1][0]) / det;
    Some(inv)
}

fn approx_rule2(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    if xs.is_empty() {
        return f64::NAN;
    }
    if x <= xs[0] {
        return ys[0];
    }
    let last = xs.len() - 1;
    if x >= xs[last] {
        return ys[last];
    }
    for i in 0..last {
        if x <= xs[i + 1] {
            let t = (x - xs[i]) / (xs[i + 1] - xs[i]);
            return ys[i] + t * (ys[i + 1] - ys[i]);
        }
    }
    ys[last]
}

/// GNU `PP.test(x)` Phillips-Perron unit-root test.
pub unsafe fn do_pp_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || XLENGTH(x) < 4 {
            return R_NilValue();
        }
        let n0 = XLENGTH(x) as usize;
        let mut xs = Vec::with_capacity(n0);
        for i in 0..n0 {
            xs.push(elt_real_safe(x, i as i64));
        }
        let n = n0 - 1;
        let mut xtx = [[0.0; 3]; 3];
        let mut xty = [0.0; 3];
        let mut yt1s = Vec::with_capacity(n);
        let mut yts = Vec::with_capacity(n);
        for i in 0..n {
            let yt = xs[i + 1];
            let yt1 = xs[i];
            let u = (i as f64 + 1.0) - (n as f64) / 2.0;
            let row = [1.0, u, yt1];
            yts.push(yt);
            yt1s.push(yt1);
            for a in 0..3 {
                xty[a] += row[a] * yt;
                for b in 0..3 {
                    xtx[a][b] += row[a] * row[b];
                }
            }
        }
        let Some(inv) = invert3(xtx) else {
            return R_NilValue();
        };
        let mut beta = [0.0; 3];
        for i in 0..3 {
            beta[i] = inv[i][0] * xty[0] + inv[i][1] * xty[1] + inv[i][2] * xty[2];
        }
        let mut resid = vec![0.0; n];
        let mut sse = 0.0;
        for i in 0..n {
            let u = (i as f64 + 1.0) - (n as f64) / 2.0;
            let fit = beta[0] + beta[1] * u + beta[2] * yt1s[i];
            let e = yts[i] - fit;
            resid[i] = e;
            sse += e * e;
        }
        let sigma2 = sse / ((n - 3) as f64);
        let se = (sigma2 * inv[2][2]).sqrt();
        let tstat = (beta[2] - 1.0) / se;
        let ssqru = sse / (n as f64);
        let l = (4.0 * (n as f64 / 100.0).powf(0.25)).trunc() as i32;
        let ssqrtl = ssqru + crate::library::stats::ppsum::r_pp_sum(&resid, l);
        let n_f = n as f64;
        let n2 = n_f * n_f;
        let mut sum_yt1_2 = 0.0;
        let mut sum_yt1_t = 0.0;
        let mut sum_yt1 = 0.0;
        for i in 0..n {
            let t = (i as f64) + 1.0;
            sum_yt1_2 += yt1s[i] * yt1s[i];
            sum_yt1_t += yt1s[i] * t;
            sum_yt1 += yt1s[i];
        }
        let trm1 = n2 * (n2 - 1.0) * sum_yt1_2 / 12.0;
        let trm2 = n_f * sum_yt1_t * sum_yt1_t;
        let trm3 = n_f * (n_f + 1.0) * sum_yt1_t * sum_yt1;
        let trm4 = (n_f * (n_f + 1.0) * (2.0 * n_f + 1.0) * sum_yt1 * sum_yt1) / 6.0;
        let dx = trm1 - trm2 + trm3 - trm4;
        let stat = ssqru.sqrt() / ssqrtl.sqrt() * tstat
            - (n_f * n_f * n_f) / (4.0 * 3.0_f64.sqrt() * dx.sqrt() * ssqrtl.sqrt())
                * (ssqrtl - ssqru);
        let table_t = [25.0, 50.0, 100.0, 250.0, 500.0, 1e5];
        let table = [
            [4.38, 4.15, 4.04, 3.99, 3.98, 3.96],
            [3.95, 3.80, 3.73, 3.69, 3.68, 3.66],
            [3.60, 3.50, 3.45, 3.43, 3.42, 3.41],
            [3.24, 3.18, 3.15, 3.13, 3.13, 3.12],
            [1.14, 1.19, 1.22, 1.23, 1.24, 1.25],
            [0.80, 0.87, 0.90, 0.92, 0.93, 0.94],
            [0.50, 0.58, 0.62, 0.64, 0.65, 0.66],
            [0.15, 0.24, 0.28, 0.31, 0.32, 0.33],
        ];
        let tablep = [0.01, 0.025, 0.05, 0.1, 0.9, 0.95, 0.975, 0.99];
        let mut tableipl = [0.0; 8];
        for i in 0..8 {
            let col: Vec<f64> = table[i].iter().map(|v| -v).collect();
            tableipl[i] = approx_rule2(&table_t, &col, n_f);
        }
        let pval = approx_rule2(&tableipl, &tablep, stat);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Dickey-Fuller".to_string()]);
        let parameter = Rf_ScalarReal(l as f64);
        let _pa = protect(parameter);
        set_string_names(parameter, &["Truncation lag parameter".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(
            result,
            3,
            Rf_mkString(c"Phillips-Perron Unit Root Test".as_ptr()),
        );
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x".as_ptr()));
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

fn matmul_p2(x: &[f64], tt: [[f64; 2]; 2], p: usize) -> Vec<f64> {
    let mut z = vec![0.0; p * 2];
    for i in 0..p {
        let a = x[i];
        let b = x[i + p];
        z[i] = a * tt[0][0] + b * tt[1][0];
        z[i + p] = a * tt[0][1] + b * tt[1][1];
    }
    z
}

fn svd2_uvt(b00: f64, b10: f64, b01: f64, b11: f64) -> ([[f64; 2]; 2], f64) {
    // B is 2x2 column-major: [b00, b10; b01, b11]
    let a = b00 * b00 + b10 * b10;
    let c = b00 * b01 + b10 * b11;
    let bb = b01 * b01 + b11 * b11;
    let disc = ((a - bb) * (a - bb) + 4.0 * c * c).sqrt();
    let l1 = 0.5 * (a + bb + disc);
    let l2 = 0.5 * (a + bb - disc);
    let (v00, v10) = if c.abs() > 1e-18 {
        let mut x = c;
        let mut y = l1 - a;
        let n = (x * x + y * y).sqrt();
        x /= n;
        y /= n;
        (x, y)
    } else if a >= bb {
        (1.0, 0.0)
    } else {
        (0.0, 1.0)
    };
    let v01 = -v10;
    let v11 = v00;
    let s1 = l1.max(0.0).sqrt();
    let s2 = l2.max(0.0).sqrt();
    let mut u00 = b00 * v00 + b01 * v10;
    let mut u10 = b10 * v00 + b11 * v10;
    let mut u01 = b00 * v01 + b01 * v11;
    let mut u11 = b10 * v01 + b11 * v11;
    if s1 > 1e-18 {
        u00 /= s1;
        u10 /= s1;
    }
    if s2 > 1e-18 {
        u01 /= s2;
        u11 /= s2;
    }
    // TT = U %*% V^T
    let t00 = u00 * v00 + u01 * v01;
    let t01 = u00 * v10 + u01 * v11;
    let t10 = u10 * v00 + u11 * v01;
    let t11 = u10 * v10 + u11 * v11;
    ([[t00, t01], [t10, t11]], s1 + s2)
}

fn varimax_2col(x: &[f64], p: usize) -> (Vec<f64>, [[f64; 2]; 2]) {
    let mut work = vec![0.0; p * 2];
    let mut sc = vec![1.0; p];
    for i in 0..p {
        let a = x[i];
        let b = x[i + p];
        sc[i] = (a * a + b * b).sqrt();
        if sc[i] > 0.0 {
            work[i] = a / sc[i];
            work[i + p] = b / sc[i];
        } else {
            work[i] = a;
            work[i + p] = b;
        }
    }
    let mut tt = [[1.0, 0.0], [0.0, 1.0]];
    let mut d = 0.0;
    let pf = p as f64;
    for _ in 0..1000 {
        let z = matmul_p2(&work, tt, p);
        let mut c1 = 0.0;
        let mut c2 = 0.0;
        for i in 0..p {
            c1 += z[i] * z[i];
            c2 += z[i + p] * z[i + p];
        }
        c1 /= pf;
        c2 /= pf;
        let mut b00 = 0.0;
        let mut b10 = 0.0;
        let mut b01 = 0.0;
        let mut b11 = 0.0;
        for i in 0..p {
            let z1 = z[i];
            let z2 = z[i + p];
            let w1 = z1 * z1 * z1 - z1 * c1;
            let w2 = z2 * z2 * z2 - z2 * c2;
            b00 += work[i] * w1;
            b10 += work[i + p] * w1;
            b01 += work[i] * w2;
            b11 += work[i + p] * w2;
        }
        let (new_tt, dn) = svd2_uvt(b00, b10, b01, b11);
        let dpast = d;
        d = dn;
        tt = new_tt;
        if d < dpast * (1.0 + 1e-5) {
            break;
        }
    }
    let mut z = matmul_p2(&work, tt, p);
    for i in 0..p {
        z[i] *= sc[i];
        z[i + p] *= sc[i];
    }
    (z, tt)
}

/// GNU 2-column `varimax(x)`.
pub unsafe fn do_varimax(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (p, nc) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            return R_NilValue();
        };
        if nc != 2 || p < 2 {
            return R_NilValue();
        }
        let mut raw = vec![0.0; p * 2];
        for i in 0..(p * 2) {
            raw[i] = elt_real_safe(x, i as i64);
        }
        let (z, tt) = varimax_2col(&raw, p);
        let loadings =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), p as i32, 2);
        let _ld = protect(loadings);
        for i in 0..(p * 2) {
            *REAL(loadings).add(i) = z[i];
        }
        let class = Rf_mkString(c"loadings".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            loadings,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        let rot = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 2, 2);
        let _rt = protect(rot);
        *REAL(rot) = tt[0][0];
        *REAL(rot).add(1) = tt[1][0];
        *REAL(rot).add(2) = tt[0][1];
        *REAL(rot).add(3) = tt[1][1];
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, loadings);
        SET_VECTOR_ELT(result, 1, rot);
        crate::mainutils::essentials::set_string_names(
            result,
            &["loadings".to_string(), "rotmat".to_string()],
        );
        result
    }
}

pub(super) fn invert2(a00: f64, a10: f64, a01: f64, a11: f64) -> Option<(f64, f64, f64, f64)> {
    let det = a00 * a11 - a01 * a10;
    if !det.is_finite() || det.abs() < 1e-18 {
        return None;
    }
    Some((a11 / det, -a10 / det, -a01 / det, a00 / det))
}

/// GNU 2-column `promax(x, m=4)`.
pub unsafe fn do_promax(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (p, nc) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            return R_NilValue();
        };
        if nc != 2 || p < 2 {
            return R_NilValue();
        }
        let mut raw = vec![0.0; p * 2];
        for i in 0..(p * 2) {
            raw[i] = elt_real_safe(x, i as i64);
        }
        let (vx, vrot) = varimax_2col(&raw, p);
        // Q = x * |x|^3  (m=4)
        let mut q = vec![0.0; p * 2];
        for i in 0..(p * 2) {
            q[i] = vx[i] * vx[i].abs().powi(3);
        }
        // U = (X'X)^{-1} X'Q
        let mut xtx = [0.0; 4];
        let mut xtq = [0.0; 4];
        for i in 0..p {
            let x1 = vx[i];
            let x2 = vx[i + p];
            let q1 = q[i];
            let q2 = q[i + p];
            xtx[0] += x1 * x1;
            xtx[1] += x2 * x1;
            xtx[2] += x1 * x2;
            xtx[3] += x2 * x2;
            xtq[0] += x1 * q1;
            xtq[1] += x2 * q1;
            xtq[2] += x1 * q2;
            xtq[3] += x2 * q2;
        }
        let Some(inv) = invert2(xtx[0], xtx[1], xtx[2], xtx[3]) else {
            return R_NilValue();
        };
        // U column-major
        let u00 = inv.0 * xtq[0] + inv.2 * xtq[1];
        let u10 = inv.1 * xtq[0] + inv.3 * xtq[1];
        let u01 = inv.0 * xtq[2] + inv.2 * xtq[3];
        let u11 = inv.1 * xtq[2] + inv.3 * xtq[3];
        // d = diag(solve(t(U) U))
        let tuu00 = u00 * u00 + u10 * u10;
        let tuu10 = u00 * u01 + u10 * u11;
        let tuu01 = tuu10;
        let tuu11 = u01 * u01 + u11 * u11;
        let Some(itu) = invert2(tuu00, tuu10, tuu01, tuu11) else {
            return R_NilValue();
        };
        let s0 = itu.0.max(0.0).sqrt();
        let s1 = itu.3.max(0.0).sqrt();
        let u00s = u00 * s0;
        let u10s = u10 * s0;
        let u01s = u01 * s1;
        let u11s = u11 * s1;
        let z = matmul_p2(&vx, [[u00s, u01s], [u10s, u11s]], p);
        let r00 = vrot[0][0] * u00s + vrot[0][1] * u10s;
        let r01 = vrot[0][0] * u01s + vrot[0][1] * u11s;
        let r10 = vrot[1][0] * u00s + vrot[1][1] * u10s;
        let r11 = vrot[1][0] * u01s + vrot[1][1] * u11s;
        let loadings =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), p as i32, 2);
        let _ld = protect(loadings);
        for i in 0..(p * 2) {
            *REAL(loadings).add(i) = z[i];
        }
        let class = Rf_mkString(c"loadings".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            loadings,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        let rot = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 2, 2);
        let _rt = protect(rot);
        *REAL(rot) = r00;
        *REAL(rot).add(1) = r10;
        *REAL(rot).add(2) = r01;
        *REAL(rot).add(3) = r11;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, loadings);
        SET_VECTOR_ELT(result, 1, rot);
        crate::mainutils::essentials::set_string_names(
            result,
            &["loadings".to_string(), "rotmat".to_string()],
        );
        result
    }
}

/// GNU `loadings(x)` — extract `$loadings`.
pub unsafe fn do_loadings(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
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
            if std::ffi::CStr::from_ptr(raw).to_string_lossy() == "loadings" {
                return VECTOR_ELT(x, i);
            }
        }
        R_NilValue()
    }
}
