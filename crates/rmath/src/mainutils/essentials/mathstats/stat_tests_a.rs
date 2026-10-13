//! Classical significance tests (t, binom, poisson, chisq, cor, var, bartlett, kruskal, ...) — extracted verbatim from essentials/mathstats.rs.

use super::*;

/// GNU one-sample `t.test(x)`.
pub unsafe fn do_t_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n0 = XLENGTH(x);
        let t = TYPEOF(x);
        let mut vals = Vec::new();
        for i in 0..n0 {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            if v.is_finite() {
                vals.push(v);
            }
        }
        let n = vals.len() as f64;
        if n < 2.0 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "not enough 'x' observations",
            );
        }
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n - 1.0);
        let s = var.sqrt();
        let se = s / n.sqrt();
        let stat = mean / se;
        let df = n - 1.0;
        let p = 2.0 * crate::dist::t_dist::pt_inner(-stat.abs(), df, true, false);
        let crit = crate::dist::t_dist::qt_inner(0.975, df, true, false);
        let lo = mean - crit * se;
        let hi = mean + crit * se;

        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["t".to_string()]);
        let parameter = Rf_ScalarReal(df);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let estimate = Rf_ScalarReal(mean);
        let _es = protect(estimate);
        set_string_names(estimate, &["mean of x".to_string()]);
        let null_value = Rf_ScalarReal(0.0);
        let _nv = protect(null_value);
        set_string_names(null_value, &["mean".to_string()]);
        let conf = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _cf = protect(conf);
        *REAL(conf) = lo;
        *REAL(conf).add(1) = hi;
        crate::sexp::attrib_core::setAttrib(
            conf,
            crate::sexp::symbol::Rf_install(c"conf.level".as_ptr()),
            Rf_ScalarReal(0.95),
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 10);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(p));
        SET_VECTOR_ELT(result, 3, conf);
        SET_VECTOR_ELT(result, 4, estimate);
        SET_VECTOR_ELT(result, 5, null_value);
        SET_VECTOR_ELT(result, 6, Rf_ScalarReal(se));
        SET_VECTOR_ELT(result, 7, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(result, 8, Rf_mkString(c"One Sample t-test".as_ptr()));
        SET_VECTOR_ELT(result, 9, Rf_mkString(c"x".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "conf.int".to_string(),
                "estimate".to_string(),
                "null.value".to_string(),
                "stderr".to_string(),
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

fn binom_two_sided_p(x: f64, n: f64, p: f64) -> f64 {
    let dobs = crate::dist::binomial::dbinom_inner(x, n, p, false);
    let mut s = 0.0;
    let mut k = 0.0;
    while k <= n + 0.5 {
        let dk = crate::dist::binomial::dbinom_inner(k, n, p, false);
        if dk <= dobs * (1.0 + 1e-7) {
            s += dk;
        }
        k += 1.0;
    }
    s.min(1.0)
}

/// GNU `binom.test(x, n, p=0.5)`.
pub unsafe fn do_binom_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = elt_real_safe(CAR(args), 0).round();
        let n_arg = CAR(CDR(args));
        let n = if n_arg.is_null() || n_arg == R_NilValue() {
            x
        } else {
            elt_real_safe(n_arg, 0).round()
        };
        let mut p0 = 0.5;
        let rest = CDR(CDR(args));
        if !rest.is_null() && rest != R_NilValue() {
            let p_s = CAR(rest);
            if !p_s.is_null() && p_s != R_NilValue() && TYPEOF(p_s) != SEXPTYPE::STRSXP {
                p0 = elt_real_safe(p_s, 0);
            }
        }
        let pval = binom_two_sided_p(x, n, p0);
        let alpha = 0.025;
        let lo = if x == 0.0 {
            0.0
        } else {
            crate::dist::beta::qbeta_inner(alpha, x, n - x + 1.0, true, false)
        };
        let hi = if x == n {
            1.0
        } else {
            crate::dist::beta::qbeta_inner(1.0 - alpha, x + 1.0, n - x, true, false)
        };
        let statistic = Rf_ScalarReal(x);
        let _st = protect(statistic);
        set_string_names(statistic, &["number of successes".to_string()]);
        let parameter = Rf_ScalarReal(n);
        let _pa = protect(parameter);
        set_string_names(parameter, &["number of trials".to_string()]);
        let estimate = Rf_ScalarReal(x / n);
        let _es = protect(estimate);
        set_string_names(estimate, &["probability of success".to_string()]);
        let null_value = Rf_ScalarReal(p0);
        let _nv = protect(null_value);
        set_string_names(null_value, &["probability of success".to_string()]);
        let conf = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _cf = protect(conf);
        *REAL(conf) = lo;
        *REAL(conf).add(1) = hi;
        crate::sexp::attrib_core::setAttrib(
            conf,
            crate::sexp::symbol::Rf_install(c"conf.level".as_ptr()),
            Rf_ScalarReal(0.95),
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 9);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, conf);
        SET_VECTOR_ELT(result, 4, estimate);
        SET_VECTOR_ELT(result, 5, null_value);
        SET_VECTOR_ELT(result, 6, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(result, 7, Rf_mkString(c"Exact binomial test".as_ptr()));
        SET_VECTOR_ELT(result, 8, Rf_mkString(c"x and n".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "conf.int".to_string(),
                "estimate".to_string(),
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

/// GNU `poisson.test(x, T=1, r=1)` — one-sample exact Poisson rate.
pub unsafe fn do_poisson_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = elt_real_safe(CAR(args), 0).round();
        let mut t = 1.0;
        let mut r = 1.0;
        let mut alt = "two.sided";
        let mut cell = CDR(args);
        let mut pos = 1usize;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                let s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(v, 0)))
                    .to_string_lossy()
                    .into_owned();
                if s == "less" || s == "greater" || s == "two.sided" {
                    alt = if s == "less" {
                        "less"
                    } else if s == "greater" {
                        "greater"
                    } else {
                        "two.sided"
                    };
                }
            } else if pos == 1 {
                t = elt_real_safe(v, 0);
            } else if pos == 2 {
                r = elt_real_safe(v, 0);
            }
            pos += 1;
            cell = CDR(cell);
        }
        if t <= 0.0 {
            t = 1.0;
        }
        let m = r * t;
        let p_less = crate::dist::poisson::ppois_inner(x, m, true, false);
        let p_greater = crate::dist::poisson::ppois_inner(x - 1.0, m, false, false);
        let pval = match alt {
            "less" => p_less,
            "greater" => p_greater,
            _ => (2.0 * p_less.min(p_greater)).min(1.0),
        };
        let estimate = Rf_ScalarReal(x / t);
        let _es = protect(estimate);
        set_string_names(estimate, &["event rate".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 1, estimate);
        SET_VECTOR_ELT(result, 2, Rf_mkString(c"Exact Poisson test".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "p.value".to_string(),
                "estimate".to_string(),
                "method".to_string(),
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

/// GNU `prop.trend.test(x, n)` — weighted chi-squared trend in proportions.
pub unsafe fn do_prop_trend_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = CAR(CDR(args));
        let k = XLENGTH(x).min(XLENGTH(n)) as usize;
        if k < 2 {
            return R_NilValue();
        }
        let mut xs = vec![0.0; k];
        let mut ns = vec![0.0; k];
        let mut scores = vec![0.0; k];
        let mut sum_x = 0.0;
        let mut sum_n = 0.0;
        let score_arg = CAR(CDR(CDR(args)));
        let have_scores = !score_arg.is_null()
            && score_arg != R_NilValue()
            && (TYPEOF(score_arg) == SEXPTYPE::REALSXP || TYPEOF(score_arg) == SEXPTYPE::INTSXP)
            && XLENGTH(score_arg) >= k as i64;
        for i in 0..k {
            xs[i] = elt_real_safe(x, i as i64);
            ns[i] = elt_real_safe(n, i as i64);
            scores[i] = if have_scores {
                elt_real_safe(score_arg, i as i64)
            } else {
                (i + 1) as f64
            };
            sum_x += xs[i];
            sum_n += ns[i];
        }
        let p = if sum_n > 0.0 { sum_x / sum_n } else { 0.0 };
        let pq = p * (1.0 - p);
        if pq <= 0.0 {
            return R_NilValue();
        }
        let mut w = vec![0.0; k];
        let mut y = vec![0.0; k];
        let mut sw = 0.0;
        for i in 0..k {
            w[i] = ns[i] / pq;
            y[i] = if ns[i] > 0.0 { xs[i] / ns[i] } else { 0.0 };
            sw += w[i];
        }
        if sw <= 0.0 {
            return R_NilValue();
        }
        let mut ms = 0.0;
        let mut my = 0.0;
        for i in 0..k {
            ms += w[i] * scores[i];
            my += w[i] * y[i];
        }
        ms /= sw;
        my /= sw;
        let mut sxx = 0.0;
        let mut sxy = 0.0;
        for i in 0..k {
            let ds = scores[i] - ms;
            sxx += w[i] * ds * ds;
            sxy += w[i] * ds * (y[i] - my);
        }
        let stat = if sxx > 0.0 { sxy * sxy / sxx } else { 0.0 };
        let pval = crate::dist::chisq::pchisq_inner(stat, 1.0, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["X-squared".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(1.0));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "p.value".to_string(),
                "parameter".to_string(),
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

/// GNU `chisq.test(x)` goodness-of-fit with equal p.
pub unsafe fn do_chisq_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let k = XLENGTH(x);
        let mut obs = Vec::with_capacity(k as usize);
        let mut n = 0.0;
        for i in 0..k {
            let v = elt_real_safe(x, i);
            obs.push(v);
            n += v;
        }
        let e = n / k as f64;
        let mut stat = 0.0;
        for v in &obs {
            if e > 0.0 {
                let d = *v - e;
                stat += d * d / e;
            }
        }
        let df = (k - 1) as f64;
        let pval = crate::dist::chisq::pchisq_inner(stat, df, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["X-squared".to_string()]);
        let parameter = Rf_ScalarReal(df);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let expected = Rf_allocVector3(SEXPTYPE::REALSXP, k);
        let _ex = protect(expected);
        for i in 0..k {
            *REAL(expected).add(i as usize) = e;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 6);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(
            result,
            3,
            Rf_mkString(c"Chi-squared test for given probabilities".as_ptr()),
        );
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x".as_ptr()));
        SET_VECTOR_ELT(result, 5, expected);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "method".to_string(),
                "data.name".to_string(),
                "expected".to_string(),
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

/// GNU one-sample `prop.test(x, n)`.
pub unsafe fn do_prop_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = elt_real_safe(CAR(args), 0);
        let n = elt_real_safe(CAR(CDR(args)), 0);
        let p0 = 0.5;
        let estimate = x / n;
        let yates = (0.5_f64).min((x - n * p0).abs());
        let e_s = n * p0;
        let e_f = n * (1.0 - p0);
        let stat = {
            let a = (x - e_s).abs() - yates;
            let b = ((n - x) - e_f).abs() - yates;
            a * a / e_s + b * b / e_f
        };
        let pval = crate::dist::chisq::pchisq_inner(stat, 1.0, false, false);
        let z = crate::dist::normal::qnorm5_inner(0.975, 0.0, 1.0, true, false);
        let z22n = z * z / (2.0 * n);
        let pc_u = estimate + yates / n;
        let pc_l = estimate - yates / n;
        let p_u = if pc_u >= 1.0 {
            1.0
        } else {
            (pc_u + z22n + z * (pc_u * (1.0 - pc_u) / n + z22n / (2.0 * n)).sqrt())
                / (1.0 + 2.0 * z22n)
        };
        let p_l = if pc_l <= 0.0 {
            0.0
        } else {
            (pc_l + z22n - z * (pc_l * (1.0 - pc_l) / n + z22n / (2.0 * n)).sqrt())
                / (1.0 + 2.0 * z22n)
        };
        let method = if yates > 0.0 {
            "1-sample proportions test with continuity correction"
        } else {
            "1-sample proportions test without continuity correction"
        };
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["X-squared".to_string()]);
        let parameter = Rf_ScalarReal(1.0);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let est = Rf_ScalarReal(estimate);
        let _es = protect(est);
        set_string_names(est, &["p".to_string()]);
        let null_value = Rf_ScalarReal(p0);
        let _nv = protect(null_value);
        set_string_names(null_value, &["p".to_string()]);
        let conf = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _cf = protect(conf);
        *REAL(conf) = p_l.max(0.0);
        *REAL(conf).add(1) = p_u.min(1.0);
        crate::sexp::attrib_core::setAttrib(
            conf,
            crate::sexp::symbol::Rf_install(c"conf.level".as_ptr()),
            Rf_ScalarReal(0.95),
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 9);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, est);
        SET_VECTOR_ELT(result, 4, null_value);
        SET_VECTOR_ELT(result, 5, conf);
        SET_VECTOR_ELT(result, 6, Rf_mkString(c"two.sided".as_ptr()));
        let m = CString::new(method).unwrap_or_default();
        SET_VECTOR_ELT(result, 7, Rf_mkString(m.as_ptr()));
        SET_VECTOR_ELT(result, 8, Rf_mkString(c"x out of n".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "estimate".to_string(),
                "null.value".to_string(),
                "conf.int".to_string(),
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

fn pearson_r(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len() as f64;
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let mut num = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;
    for i in 0..x.len() {
        let dx = x[i] - mx;
        let dy = y[i] - my;
        num += dx * dy;
        sx += dx * dx;
        sy += dy * dy;
    }
    num / (sx * sy).sqrt()
}

/// GNU Pearson `cor.test(x, y)`.
pub unsafe fn do_cor_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let xs = CAR(args);
        let ys = CAR(CDR(args));
        let n0 = XLENGTH(xs).min(XLENGTH(ys));
        let mut x = Vec::new();
        let mut y = Vec::new();
        for i in 0..n0 {
            let a = elt_real_safe(xs, i);
            let b = elt_real_safe(ys, i);
            if a.is_finite() && b.is_finite() {
                x.push(a);
                y.push(b);
            }
        }
        let n = x.len() as f64;
        let r = pearson_r(&x, &y);
        let df = n - 2.0;
        let stat = df.sqrt() * r / (1.0 - r * r).sqrt();
        let p1 = crate::dist::t_dist::pt_inner(stat, df, true, false);
        let p2 = crate::dist::t_dist::pt_inner(stat, df, false, false);
        let pval = (2.0 * p1.min(p2)).min(1.0);
        let z = r.atanh();
        let sigma = 1.0 / (n - 3.0).sqrt();
        let qz = crate::dist::normal::qnorm5_inner(0.975, 0.0, 1.0, true, false);
        let lo = (z - sigma * qz).tanh();
        let hi = (z + sigma * qz).tanh();
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["t".to_string()]);
        let parameter = Rf_ScalarReal(df);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let estimate = Rf_ScalarReal(r);
        let _es = protect(estimate);
        set_string_names(estimate, &["cor".to_string()]);
        let null_value = Rf_ScalarReal(0.0);
        let _nv = protect(null_value);
        set_string_names(null_value, &["correlation".to_string()]);
        let conf = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _cf = protect(conf);
        *REAL(conf) = lo;
        *REAL(conf).add(1) = hi;
        crate::sexp::attrib_core::setAttrib(
            conf,
            crate::sexp::symbol::Rf_install(c"conf.level".as_ptr()),
            Rf_ScalarReal(0.95),
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 9);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, estimate);
        SET_VECTOR_ELT(result, 4, null_value);
        SET_VECTOR_ELT(result, 5, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(
            result,
            6,
            Rf_mkString(c"Pearson's product-moment correlation".as_ptr()),
        );
        SET_VECTOR_ELT(result, 7, Rf_mkString(c"x and y".as_ptr()));
        SET_VECTOR_ELT(result, 8, conf);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "estimate".to_string(),
                "null.value".to_string(),
                "alternative".to_string(),
                "method".to_string(),
                "data.name".to_string(),
                "conf.int".to_string(),
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

fn sample_var(x: &[f64]) -> f64 {
    let n = x.len() as f64;
    let m = x.iter().sum::<f64>() / n;
    x.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (n - 1.0)
}

/// GNU `var.test(x, y)`.
pub unsafe fn do_var_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let xs = CAR(args);
        let ys = CAR(CDR(args));
        let mut x = Vec::new();
        let mut y = Vec::new();
        for i in 0..XLENGTH(xs) {
            let v = elt_real_safe(xs, i);
            if v.is_finite() {
                x.push(v);
            }
        }
        for i in 0..XLENGTH(ys) {
            let v = elt_real_safe(ys, i);
            if v.is_finite() {
                y.push(v);
            }
        }
        let dfx = (x.len() - 1) as f64;
        let dfy = (y.len() - 1) as f64;
        let vx = sample_var(&x);
        let vy = sample_var(&y);
        let est = vx / vy;
        let stat = est;
        let p_lo = crate::dist::f_dist::pf_inner(stat, dfx, dfy, true, false);
        let pval = (2.0 * p_lo.min(1.0 - p_lo)).min(1.0);
        let lo = est / crate::dist::f_dist::qf_inner(0.975, dfx, dfy, true, false);
        let hi = est / crate::dist::f_dist::qf_inner(0.025, dfx, dfy, true, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["F".to_string()]);
        let parameter = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _pa = protect(parameter);
        *REAL(parameter) = dfx;
        *REAL(parameter).add(1) = dfy;
        set_string_names(parameter, &["num df".to_string(), "denom df".to_string()]);
        let estimate = Rf_ScalarReal(est);
        let _es = protect(estimate);
        set_string_names(estimate, &["ratio of variances".to_string()]);
        let null_value = Rf_ScalarReal(1.0);
        let _nv = protect(null_value);
        set_string_names(null_value, &["ratio of variances".to_string()]);
        let conf = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _cf = protect(conf);
        *REAL(conf) = lo;
        *REAL(conf).add(1) = hi;
        crate::sexp::attrib_core::setAttrib(
            conf,
            crate::sexp::symbol::Rf_install(c"conf.level".as_ptr()),
            Rf_ScalarReal(0.95),
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 9);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, conf);
        SET_VECTOR_ELT(result, 4, estimate);
        SET_VECTOR_ELT(result, 5, null_value);
        SET_VECTOR_ELT(result, 6, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(
            result,
            7,
            Rf_mkString(c"F test to compare two variances".as_ptr()),
        );
        SET_VECTOR_ELT(result, 8, Rf_mkString(c"x and y".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "conf.int".to_string(),
                "estimate".to_string(),
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

/// GNU `bartlett.test(x, g)`.
pub unsafe fn do_bartlett_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let g = CAR(CDR(args));
        let n0 = XLENGTH(x);
        let mut groups: std::collections::BTreeMap<i32, Vec<f64>> =
            std::collections::BTreeMap::new();
        for i in 0..n0 {
            let v = elt_real_safe(x, i);
            if !v.is_finite() {
                continue;
            }
            let gi = if g.is_null() || g == R_NilValue() {
                1
            } else if TYPEOF(g) == SEXPTYPE::INTSXP || TYPEOF(g) == SEXPTYPE::LGLSXP {
                *INTEGER(g).add((i as usize) % XLENGTH(g) as usize)
            } else if TYPEOF(g) == SEXPTYPE::REALSXP {
                *REAL(g).add((i as usize) % XLENGTH(g) as usize) as i32
            } else {
                1
            };
            groups.entry(gi).or_default().push(v);
        }
        let k = groups.len();
        let mut ns: Vec<f64> = Vec::new();
        let mut vs: Vec<f64> = Vec::new();
        for vals in groups.values() {
            if vals.len() < 2 {
                continue;
            }
            ns.push((vals.len() - 1) as f64);
            vs.push(sample_var(vals));
        }
        let n_total: f64 = ns.iter().sum();
        let v_total = ns.iter().zip(vs.iter()).map(|(n, v)| n * v).sum::<f64>() / n_total;
        let num = n_total * v_total.ln()
            - ns.iter()
                .zip(vs.iter())
                .map(|(n, v)| n * v.ln())
                .sum::<f64>();
        let den = 1.0
            + (ns.iter().map(|n| 1.0 / n).sum::<f64>() - 1.0 / n_total) / (3.0 * (k as f64 - 1.0));
        let stat = num / den;
        let df = (k as f64) - 1.0;
        let pval = crate::dist::chisq::pchisq_inner(stat, df, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Bartlett's K-squared".to_string()]);
        let parameter = Rf_ScalarReal(df);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 3, Rf_mkString(c"x and g".as_ptr()));
        SET_VECTOR_ELT(
            result,
            4,
            Rf_mkString(c"Bartlett test of homogeneity of variances".as_ptr()),
        );
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "parameter".to_string(),
                "p.value".to_string(),
                "data.name".to_string(),
                "method".to_string(),
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

pub(super) fn rank_average(x: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..x.len()).collect();
    idx.sort_by(|&a, &b| x[a].partial_cmp(&x[b]).unwrap_or(std::cmp::Ordering::Equal));
    let mut ranks = vec![0.0; x.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i + 1;
        while j < idx.len() && x[idx[j]] == x[idx[i]] {
            j += 1;
        }
        let avg = ((i + 1) + j) as f64 / 2.0;
        for k in i..j {
            ranks[idx[k]] = avg;
        }
        i = j;
    }
    ranks
}

/// GNU `kruskal.test(x, g)`.
pub unsafe fn do_kruskal_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let g = CAR(CDR(args));
        let n0 = XLENGTH(x);
        let mut vals = Vec::new();
        let mut groups = Vec::new();
        for i in 0..n0 {
            let v = elt_real_safe(x, i);
            if !v.is_finite() {
                continue;
            }
            let gi = if g.is_null() || g == R_NilValue() {
                1
            } else if TYPEOF(g) == SEXPTYPE::INTSXP || TYPEOF(g) == SEXPTYPE::LGLSXP {
                *INTEGER(g).add((i as usize) % XLENGTH(g) as usize)
            } else if TYPEOF(g) == SEXPTYPE::REALSXP {
                *REAL(g).add((i as usize) % XLENGTH(g) as usize) as i32
            } else {
                1
            };
            vals.push(v);
            groups.push(gi);
        }
        let ranks = rank_average(&vals);
        let n = vals.len() as f64;
        let mut by_g: std::collections::BTreeMap<i32, (f64, f64)> =
            std::collections::BTreeMap::new();
        for (r, g) in ranks.iter().zip(groups.iter()) {
            let e = by_g.entry(*g).or_insert((0.0, 0.0));
            e.0 += *r;
            e.1 += 1.0;
        }
        let k = by_g.len();
        let sum_r2_n: f64 = by_g.values().map(|(sr, ng)| sr * sr / ng).sum();
        let mut tie_adj = 0.0;
        let mut counts: std::collections::BTreeMap<u64, f64> = std::collections::BTreeMap::new();
        for v in &vals {
            *counts.entry(v.to_bits()).or_insert(0.0) += 1.0;
        }
        for t in counts.values() {
            tie_adj += t * t * t - t;
        }
        let den = 1.0 - tie_adj / (n * n * n - n);
        let stat = (12.0 * sum_r2_n / (n * (n + 1.0)) - 3.0 * (n + 1.0)) / den;
        let df = (k as f64) - 1.0;
        let pval = crate::dist::chisq::pchisq_inner(stat, df, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Kruskal-Wallis chi-squared".to_string()]);
        let parameter = Rf_ScalarReal(df);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(
            result,
            3,
            Rf_mkString(c"Kruskal-Wallis rank sum test".as_ptr()),
        );
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

pub(super) fn median_of(x: &[f64]) -> f64 {
    let mut s = x.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = s.len();
    if n == 0 {
        return f64::NAN;
    }
    if n % 2 == 1 {
        s[n / 2]
    } else {
        0.5 * (s[n / 2 - 1] + s[n / 2])
    }
}

/// GNU `fligner.test(x, g)`.
pub unsafe fn do_fligner_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let g = CAR(CDR(args));
        let n0 = XLENGTH(x);
        let mut vals = Vec::new();
        let mut groups = Vec::new();
        for i in 0..n0 {
            let v = elt_real_safe(x, i);
            if !v.is_finite() {
                continue;
            }
            let gi = if g.is_null() || g == R_NilValue() {
                1
            } else if TYPEOF(g) == SEXPTYPE::INTSXP || TYPEOF(g) == SEXPTYPE::LGLSXP {
                *INTEGER(g).add((i as usize) % XLENGTH(g) as usize)
            } else if TYPEOF(g) == SEXPTYPE::REALSXP {
                *REAL(g).add((i as usize) % XLENGTH(g) as usize) as i32
            } else {
                1
            };
            vals.push(v);
            groups.push(gi);
        }
        let mut by_g: std::collections::BTreeMap<i32, Vec<f64>> = std::collections::BTreeMap::new();
        for (v, g) in vals.iter().zip(groups.iter()) {
            by_g.entry(*g).or_default().push(*v);
        }
        let medians: std::collections::BTreeMap<i32, f64> =
            by_g.iter().map(|(k, v)| (*k, median_of(v))).collect();
        let centered: Vec<f64> = vals
            .iter()
            .zip(groups.iter())
            .map(|(v, g)| v - medians[g])
            .collect();
        let abs_c: Vec<f64> = centered.iter().map(|v| v.abs()).collect();
        let ranks = rank_average(&abs_c);
        let n = vals.len() as f64;
        let mut a: Vec<f64> = ranks
            .iter()
            .map(|r| {
                crate::dist::normal::qnorm5_inner(
                    (1.0 + r / (n + 1.0)) / 2.0,
                    0.0,
                    1.0,
                    true,
                    false,
                )
            })
            .collect();
        let mean_a = a.iter().sum::<f64>() / n;
        for v in &mut a {
            *v -= mean_a;
        }
        let vsum = a.iter().map(|v| v * v).sum::<f64>() / (n - 1.0);
        let mut a_by_g: std::collections::BTreeMap<i32, Vec<f64>> =
            std::collections::BTreeMap::new();
        for (ai, g) in a.iter().zip(groups.iter()) {
            a_by_g.entry(*g).or_default().push(*ai);
        }
        let k = a_by_g.len();
        let stat = a_by_g
            .values()
            .map(|ag| {
                let m = ag.iter().sum::<f64>() / ag.len() as f64;
                (ag.len() as f64) * m * m
            })
            .sum::<f64>()
            / vsum;
        let df = (k as f64) - 1.0;
        let pval = crate::dist::chisq::pchisq_inner(stat, df, false, false);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Fligner-Killeen:med chi-squared".to_string()]);
        let parameter = Rf_ScalarReal(df);
        let _pa = protect(parameter);
        set_string_names(parameter, &["df".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, parameter);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(
            result,
            3,
            Rf_mkString(c"Fligner-Killeen test of homogeneity of variances".as_ptr()),
        );
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

/// GNU `mood.test(x, y)`.
pub unsafe fn do_mood_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let xs = CAR(args);
        let ys = CAR(CDR(args));
        let mut x = Vec::new();
        let mut y = Vec::new();
        for i in 0..XLENGTH(xs) {
            let v = elt_real_safe(xs, i);
            if v.is_finite() {
                x.push(v);
            }
        }
        for i in 0..XLENGTH(ys) {
            let v = elt_real_safe(ys, i);
            if v.is_finite() {
                y.push(v);
            }
        }
        let m = x.len() as f64;
        let n = y.len() as f64;
        let ntot = m + n;
        let mut z = x.clone();
        z.extend_from_slice(&y);
        let mut has_ties = false;
        let mut seen = std::collections::BTreeSet::new();
        for v in &z {
            if !seen.insert(v.to_bits()) {
                has_ties = true;
                break;
            }
        }
        let e = m * (ntot * ntot - 1.0) / 12.0;
        let mut v = m * n * (ntot + 1.0) * (ntot + 2.0) * (ntot - 2.0) / 180.0;
        let tstat = if !has_ties {
            let ranks = rank_average(&z);
            let mid = (ntot + 1.0) / 2.0;
            ranks
                .iter()
                .take(x.len())
                .map(|r| (r - mid) * (r - mid))
                .sum::<f64>()
        } else {
            let mut u = z.clone();
            u.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            u.dedup_by(|a, b| a.to_bits() == b.to_bits());
            let mut a_counts = vec![0.0; u.len()];
            let mut t_counts = vec![0.0; u.len()];
            for xv in &x {
                if let Some(j) = u.iter().position(|uv| uv.to_bits() == xv.to_bits()) {
                    a_counts[j] += 1.0;
                }
            }
            for zv in &z {
                if let Some(j) = u.iter().position(|uv| uv.to_bits() == zv.to_bits()) {
                    t_counts[j] += 1.0;
                }
            }
            let mid = (ntot + 1.0) / 2.0;
            let mut p = vec![0.0; ntot as usize];
            let mut acc = 0.0;
            for i in 0..(ntot as usize) {
                let d = (i as f64 + 1.0) - mid;
                acc += d * d;
                p[i] = acc;
            }
            let mut csum = 0.0;
            let mut p_at = Vec::new();
            let mut cums = Vec::new();
            for t in &t_counts {
                csum += *t;
                cums.push(csum);
                p_at.push(p[(csum as usize) - 1]);
            }
            let mut prev = 0.0;
            let mut tstat = 0.0;
            let mut sum_term = 0.0;
            for (j, t) in t_counts.iter().enumerate() {
                let block = p_at[j] - prev;
                prev = p_at[j];
                tstat += a_counts[j] * block / t;
                let cs = cums[j];
                let inner = t * t - 4.0 + 15.0 * (ntot - 2.0 * cs + t).powi(2);
                sum_term += t * (t * t - 1.0) * inner;
            }
            v -= (m * n) / (180.0 * ntot * (ntot - 1.0)) * sum_term;
            tstat
        };
        let zstat = (tstat - e) / v.sqrt();
        let p = crate::dist::normal::pnorm5_inner(zstat, 0.0, 1.0, true, false);
        let pval = (2.0 * p.min(1.0 - p)).min(1.0);
        let statistic = Rf_ScalarReal(zstat);
        let _st = protect(statistic);
        set_string_names(statistic, &["Z".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 2, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(
            result,
            3,
            Rf_mkString(c"Mood two-sample test of scale".as_ptr()),
        );
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"x and y".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
                "p.value".to_string(),
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

/// GNU `ansari.test(x, y)` asymptotic.
pub unsafe fn do_ansari_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let xs = CAR(args);
        let ys = CAR(CDR(args));
        let mut x = Vec::new();
        let mut y = Vec::new();
        for i in 0..XLENGTH(xs) {
            let v = elt_real_safe(xs, i);
            if v.is_finite() {
                x.push(v);
            }
        }
        for i in 0..XLENGTH(ys) {
            let v = elt_real_safe(ys, i);
            if v.is_finite() {
                y.push(v);
            }
        }
        let m = x.len() as f64;
        let n = y.len() as f64;
        let ntot = m + n;
        let mut z = x.clone();
        z.extend_from_slice(&y);
        let ranks = rank_average(&z);
        let scores: Vec<f64> = ranks.iter().map(|r| r.min(ntot - r + 1.0)).collect();
        let stat: f64 = scores.iter().take(x.len()).sum();
        let even = (ntot as i64) % 2 == 0;
        let mut unique = z.clone();
        unique.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        unique.dedup_by(|a, b| a.to_bits() == b.to_bits());
        let ties = unique.len() != z.len();
        let (zz, sigma) = if !ties {
            if even {
                (
                    stat - m * (ntot + 2.0) / 4.0,
                    (m * n * (ntot + 2.0) * (ntot - 2.0) / (48.0 * (ntot - 1.0))).sqrt(),
                )
            } else {
                (
                    stat - m * (ntot + 1.0).powi(2) / (4.0 * ntot),
                    (m * n * (ntot + 1.0) * (3.0 + ntot * ntot) / (48.0 * ntot * ntot)).sqrt(),
                )
            }
        } else {
            let mean_a = scores.iter().sum::<f64>() / ntot;
            let var_a = scores
                .iter()
                .map(|a| (a - mean_a) * (a - mean_a))
                .sum::<f64>()
                / (ntot - 1.0);
            (stat - m * mean_a, (m * n * var_a / ntot).sqrt())
        };
        let p = crate::dist::normal::pnorm5_inner(zz / sigma, 0.0, 1.0, true, false);
        let pval = (2.0 * p.min(1.0 - p)).min(1.0);
        let statistic = Rf_ScalarReal(stat);
        let _st = protect(statistic);
        set_string_names(statistic, &["AB".to_string()]);
        let null_value = Rf_ScalarReal(1.0);
        let _nv = protect(null_value);
        set_string_names(null_value, &["ratio of scales".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 6);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, statistic);
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(pval));
        SET_VECTOR_ELT(result, 2, null_value);
        SET_VECTOR_ELT(result, 3, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(result, 4, Rf_mkString(c"Ansari-Bradley test".as_ptr()));
        SET_VECTOR_ELT(result, 5, Rf_mkString(c"x and y".as_ptr()));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "statistic".to_string(),
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
