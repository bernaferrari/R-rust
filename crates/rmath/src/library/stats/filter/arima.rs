//! AR/ARIMA fitting: Yule-Walker/Burg/OLS/MLE drivers, CSS kernels, arima/arima0/sim

use super::*;

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

fn golden_min(mut lo: f64, mut hi: f64, steps: usize, mut f: impl FnMut(f64) -> f64) -> f64 {
    let gr = (5.0_f64.sqrt() - 1.0) / 2.0;
    let mut c = hi - gr * (hi - lo);
    let mut d = lo + gr * (hi - lo);
    let mut fc = f(c);
    let mut fd = f(d);
    for _ in 0..steps {
        if fc < fd {
            hi = d;
            d = c;
            fd = fc;
            c = hi - gr * (hi - lo);
            fc = f(c);
        } else {
            lo = c;
            c = d;
            fc = fd;
            d = lo + gr * (hi - lo);
            fd = f(d);
        }
    }
    0.5 * (lo + hi)
}

/// GNU `ARIMA_CSS` for no seasonal difference.
fn arma_css(y: &[f64], phi: &[f64], theta: &[f64], intercept: f64, ncond: usize) -> f64 {
    let n = y.len();
    if ncond >= n {
        return f64::INFINITY;
    }
    let mut resid = vec![0.0; n];
    let mut ssq = 0.0;
    let mut nu = 0.0;
    for l in ncond..n {
        let mut tmp = y[l] - intercept;
        for (j, &pj) in phi.iter().enumerate() {
            if l > j {
                tmp -= pj * (y[l - j - 1] - intercept);
            }
        }
        let maxq = (l - ncond).min(theta.len());
        for j in 0..maxq {
            tmp -= theta[j] * resid[l - j - 1];
        }
        resid[l] = tmp;
        if tmp.is_finite() {
            ssq += tmp * tmp;
            nu += 1.0;
        }
    }
    if nu < 1.0 { f64::INFINITY } else { ssq / nu }
}

fn css_ar0(y: &[f64]) -> (f64, f64) {
    let n = y.len() as f64;
    if n < 1.0 {
        return (0.0, f64::NAN);
    }
    let mu = y.iter().sum::<f64>() / n;
    let s2 = y.iter().map(|v| (v - mu) * (v - mu)).sum::<f64>() / n;
    (mu, s2)
}

fn css_ma1(y: &[f64]) -> (f64, f64, f64) {
    let n = y.len();
    if n < 2 {
        return (0.0, 0.0, f64::NAN);
    }
    let mut ic = y.iter().sum::<f64>() / n as f64;
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    let mut th = 0.0;
    for _ in 0..20 {
        th = golden_min(-0.99, 0.99, 80, |t| arma_css(y, &[], &[t], ic, 0));
        ic = golden_min(lo_ic, hi_ic, 80, |m| arma_css(y, &[], &[th], m, 0));
    }
    let s2 = arma_css(y, &[], &[th], ic, 0);
    (th, ic, s2)
}

fn poly_expand_ar(ar: &[f64], sar: &[f64], period: usize) -> Vec<f64> {
    if sar.is_empty() {
        return ar.to_vec();
    }
    let mut out = vec![0.0; ar.len() + sar.len() * period];
    for (i, &a) in ar.iter().enumerate() {
        out[i] = a;
    }
    for (j, &s) in sar.iter().enumerate() {
        out[period * (j + 1) - 1] += s;
        for (i, &a) in ar.iter().enumerate() {
            out[period * (j + 1) + i] -= a * s;
        }
    }
    out
}

fn poly_expand_ma(ma: &[f64], sma: &[f64], period: usize) -> Vec<f64> {
    if sma.is_empty() {
        return ma.to_vec();
    }
    let mut out = vec![0.0; ma.len() + sma.len() * period];
    for (i, &a) in ma.iter().enumerate() {
        out[i] = a;
    }
    for (j, &s) in sma.iter().enumerate() {
        out[period * (j + 1) - 1] += s;
        for (i, &a) in ma.iter().enumerate() {
            out[period * (j + 1) + i] += a * s;
        }
    }
    out
}

fn arima_ss_from(phi: &[f64], theta: &[f64]) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
    let r = phi.len().max(theta.len() + 1).max(1);
    let mut t = vec![0.0; r * r];
    for (i, &a) in phi.iter().enumerate() {
        if i < r {
            t[i] = a;
        }
    }
    for ind in 1..r {
        t[(ind - 1) + ind * r] = 1.0;
    }
    let mut rr = vec![0.0; r];
    rr[0] = 1.0;
    for (i, &a) in theta.iter().enumerate() {
        if i + 1 < r {
            rr[i + 1] = a;
        }
    }
    let mut v = vec![0.0; r * r];
    for i in 0..r {
        for j in 0..r {
            v[i + j * r] = rr[i] * rr[j];
        }
    }
    let pn = lyapunov_nd(&t, &v, r);
    let mut z = vec![0.0; r];
    z[0] = 1.0;
    (t, v, pn, z)
}

fn arima_ml_nll(
    y: &[f64],
    ar: &[f64],
    ma: &[f64],
    sar: &[f64],
    sma: &[f64],
    mu: f64,
    period: usize,
) -> f64 {
    let phi = poly_expand_ar(ar, sar, period);
    let theta = poly_expand_ma(ma, sma, period);
    let (t, v, pn, z) = arima_ss_from(&phi, &theta);
    let a0 = vec![0.0; z.len()];
    let yd: Vec<f64> = y.iter().map(|v| v - mu).collect();
    kalman_like_nd(&yd, &z, &a0, &pn, &t, &v, 0.0, 0)
}

fn exact_arima_ml(
    y: &[f64],
    p: usize,
    q: usize,
    sar_p: usize,
    sar_q: usize,
    period: usize,
    include_mean: bool,
) -> (Vec<f64>, Vec<String>, f64) {
    let n = y.len();
    if n < p + q + 2 {
        return (vec![], vec![], f64::NAN);
    }
    let mut ar = vec![0.0; p];
    let mut ma = vec![0.0; q];
    let mut sar = vec![0.0; sar_p];
    let mut sma = vec![0.0; sar_q];
    let mut ic = if include_mean {
        y.iter().sum::<f64>() / n as f64
    } else {
        0.0
    };
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    let ncond = p + sar_p * period.max(1);
    let css = |ar: &[f64], ma: &[f64], sar: &[f64], sma: &[f64], mu: f64| {
        let phi = poly_expand_ar(ar, sar, period);
        let theta = poly_expand_ma(ma, sma, period);
        arma_css(y, &phi, &theta, mu, ncond)
    };
    for _ in 0..15 {
        for i in 0..p {
            ar[i] = golden_min(-0.99, 0.99, 80, |a| {
                let mut t = ar.clone();
                t[i] = a;
                css(&t, &ma, &sar, &sma, ic)
            });
        }
        for i in 0..q {
            ma[i] = golden_min(-0.99, 0.99, 80, |a| {
                let mut t = ma.clone();
                t[i] = a;
                css(&ar, &t, &sar, &sma, ic)
            });
        }
        for i in 0..sar_p {
            sar[i] = golden_min(-0.99, 0.99, 80, |a| {
                let mut t = sar.clone();
                t[i] = a;
                css(&ar, &ma, &t, &sma, ic)
            });
        }
        for i in 0..sar_q {
            sma[i] = golden_min(-0.99, 0.99, 80, |a| {
                let mut t = sma.clone();
                t[i] = a;
                css(&ar, &ma, &sar, &t, ic)
            });
        }
        if include_mean {
            ic = golden_min(lo_ic, hi_ic, 80, |m| css(&ar, &ma, &sar, &sma, m));
        }
    }
    let nll = |ar: &[f64], ma: &[f64], sar: &[f64], sma: &[f64], mu: f64| {
        arima_ml_nll(y, ar, ma, sar, sma, mu, period)
    };
    for _ in 0..25 {
        for i in 0..p {
            ar[i] = golden_min(-0.99, 0.99, 80, |a| {
                let mut t = ar.clone();
                t[i] = a;
                nll(&t, &ma, &sar, &sma, ic)
            });
        }
        for i in 0..q {
            ma[i] = golden_min(-0.99, 0.99, 80, |a| {
                let mut t = ma.clone();
                t[i] = a;
                nll(&ar, &t, &sar, &sma, ic)
            });
        }
        for i in 0..sar_p {
            sar[i] = golden_min(-0.99, 0.99, 80, |a| {
                let mut t = sar.clone();
                t[i] = a;
                nll(&ar, &ma, &t, &sma, ic)
            });
        }
        for i in 0..sar_q {
            sma[i] = golden_min(-0.99, 0.99, 80, |a| {
                let mut t = sma.clone();
                t[i] = a;
                nll(&ar, &ma, &sar, &t, ic)
            });
        }
        if include_mean {
            ic = golden_min(lo_ic, hi_ic, 80, |m| nll(&ar, &ma, &sar, &sma, m));
        }
    }
    let phi = poly_expand_ar(&ar, &sar, period);
    let theta = poly_expand_ma(&ma, &sma, period);
    let (t, v, pn, z) = arima_ss_from(&phi, &theta);
    let a0 = vec![0.0; z.len()];
    let yd: Vec<f64> = y.iter().map(|v| v - ic).collect();
    let s2 = kalman_s2_nd(&yd, &z, &a0, &pn, &t, &v, 0.0, 0);
    let mut values = Vec::new();
    let mut names = Vec::new();
    for i in 0..p {
        values.push(ar[i]);
        names.push(format!("ar{}", i + 1));
    }
    for i in 0..q {
        values.push(ma[i]);
        names.push(format!("ma{}", i + 1));
    }
    for i in 0..sar_p {
        values.push(sar[i]);
        names.push(format!("sar{}", i + 1));
    }
    for i in 0..sar_q {
        values.push(sma[i]);
        names.push(format!("sma{}", i + 1));
    }
    if include_mean {
        values.push(ic);
        names.push("intercept".to_string());
    }
    (values, names, s2)
}

fn css_arma11(y: &[f64]) -> (f64, f64, f64, f64) {
    let n = y.len();
    if n < 3 {
        return (0.0, 0.0, 0.0, f64::NAN);
    }
    let mut phi = 0.0;
    let mut th = 0.0;
    let mut ic = y.iter().sum::<f64>() / n as f64;
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    for _ in 0..25 {
        phi = golden_min(-0.99, 0.99, 80, |p| arma_css(y, &[p], &[th], ic, 1));
        th = golden_min(-0.99, 0.99, 80, |t| arma_css(y, &[phi], &[t], ic, 1));
        ic = golden_min(lo_ic, hi_ic, 80, |m| arma_css(y, &[phi], &[th], m, 1));
    }
    let s2 = arma_css(y, &[phi], &[th], ic, 1);
    (phi, th, ic, s2)
}

fn css_sar1(y: &[f64], period: usize) -> (f64, f64, f64) {
    let n = y.len();
    if period < 2 || n <= period {
        return (0.0, 0.0, f64::NAN);
    }
    let mut phi = vec![0.0; period];
    let mut sar = 0.0;
    let mut ic = y.iter().sum::<f64>() / n as f64;
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    for _ in 0..25 {
        sar = golden_min(-0.99, 0.99, 80, |p| {
            phi[period - 1] = p;
            arma_css(y, &phi, &[], ic, period)
        });
        phi[period - 1] = sar;
        ic = golden_min(lo_ic, hi_ic, 80, |m| arma_css(y, &phi, &[], m, period));
    }
    let s2 = arma_css(y, &phi, &[], ic, period);
    (sar, ic, s2)
}

fn css_sma1(y: &[f64], period: usize) -> (f64, f64, f64) {
    let n = y.len();
    if period < 2 || n <= period {
        return (0.0, 0.0, f64::NAN);
    }
    let mut theta = vec![0.0; period];
    let mut sma = 0.0;
    let mut ic = y.iter().sum::<f64>() / n as f64;
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    for _ in 0..25 {
        sma = golden_min(-0.99, 0.99, 80, |t| {
            theta[period - 1] = t;
            arma_css(y, &[], &theta, ic, 0)
        });
        theta[period - 1] = sma;
        ic = golden_min(lo_ic, hi_ic, 80, |m| arma_css(y, &[], &theta, m, 0));
    }
    let s2 = arma_css(y, &[], &theta, ic, 0);
    (sma, ic, s2)
}

fn css_ar1_sar1(y: &[f64], period: usize) -> (f64, f64, f64, f64) {
    let n = y.len();
    let ncond = 1 + period;
    if period < 2 || n <= ncond {
        return (0.0, 0.0, 0.0, f64::NAN);
    }
    let mut ar = 0.0;
    let mut sar = 0.0;
    let mut ic = y.iter().sum::<f64>() / n as f64;
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    let expand = |ar: f64, sar: f64| -> Vec<f64> {
        let mut phi = vec![0.0; period + 1];
        phi[0] = ar;
        phi[period - 1] = sar;
        phi[period] = -ar * sar;
        phi
    };
    for _ in 0..25 {
        ar = golden_min(-0.99, 0.99, 80, |a| {
            arma_css(y, &expand(a, sar), &[], ic, ncond)
        });
        sar = golden_min(-0.99, 0.99, 80, |s| {
            arma_css(y, &expand(ar, s), &[], ic, ncond)
        });
        ic = golden_min(lo_ic, hi_ic, 80, |m| {
            arma_css(y, &expand(ar, sar), &[], m, ncond)
        });
    }
    let s2 = arma_css(y, &expand(ar, sar), &[], ic, ncond);
    (ar, sar, ic, s2)
}

fn css_ma1_sma1(y: &[f64], period: usize) -> (f64, f64, f64, f64) {
    let n = y.len();
    if period < 2 || n <= period + 1 {
        return (0.0, 0.0, 0.0, f64::NAN);
    }
    let mut ma = 0.0;
    let mut sma = 0.0;
    let mut ic = y.iter().sum::<f64>() / n as f64;
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    let expand = |ma: f64, sma: f64| -> Vec<f64> {
        let mut theta = vec![0.0; period + 1];
        theta[0] = ma;
        theta[period - 1] = sma;
        theta[period] = ma * sma;
        theta
    };
    for _ in 0..25 {
        ma = golden_min(-0.99, 0.99, 80, |a| {
            arma_css(y, &[], &expand(a, sma), ic, 0)
        });
        sma = golden_min(-0.99, 0.99, 80, |s| arma_css(y, &[], &expand(ma, s), ic, 0));
        ic = golden_min(lo_ic, hi_ic, 80, |m| {
            arma_css(y, &[], &expand(ma, sma), m, 0)
        });
    }
    let s2 = arma_css(y, &[], &expand(ma, sma), ic, 0);
    (ma, sma, ic, s2)
}

fn css_ar1_sma1(y: &[f64], period: usize) -> (f64, f64, f64, f64) {
    let n = y.len();
    if period < 2 || n <= period + 1 {
        return (0.0, 0.0, 0.0, f64::NAN);
    }
    let mut ar = 0.0;
    let mut sma = 0.0;
    let mut ic = y.iter().sum::<f64>() / n as f64;
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    let ncond = 1usize;
    for _ in 0..25 {
        let mut theta = vec![0.0; period];
        theta[period - 1] = sma;
        ar = golden_min(-0.99, 0.99, 80, |a| arma_css(y, &[a], &theta, ic, ncond));
        sma = golden_min(-0.99, 0.99, 80, |s| {
            let mut th = vec![0.0; period];
            th[period - 1] = s;
            arma_css(y, &[ar], &th, ic, ncond)
        });
        theta[period - 1] = sma;
        ic = golden_min(lo_ic, hi_ic, 80, |m| arma_css(y, &[ar], &theta, m, ncond));
    }
    let mut theta = vec![0.0; period];
    theta[period - 1] = sma;
    let s2 = arma_css(y, &[ar], &theta, ic, ncond);
    (ar, sma, ic, s2)
}

fn css_ma1_sar1(y: &[f64], period: usize) -> (f64, f64, f64, f64) {
    let n = y.len();
    if period < 2 || n <= period {
        return (0.0, 0.0, 0.0, f64::NAN);
    }
    let mut ma = 0.0;
    let mut sar = 0.0;
    let mut ic = y.iter().sum::<f64>() / n as f64;
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    let ncond = period;
    let phi = |sar: f64| -> Vec<f64> {
        let mut p = vec![0.0; period];
        p[period - 1] = sar;
        p
    };
    for _ in 0..25 {
        ma = golden_min(-0.99, 0.99, 80, |a| arma_css(y, &phi(sar), &[a], ic, ncond));
        sar = golden_min(-0.99, 0.99, 80, |s| arma_css(y, &phi(s), &[ma], ic, ncond));
        ic = golden_min(lo_ic, hi_ic, 80, |m| {
            arma_css(y, &phi(sar), &[ma], m, ncond)
        });
    }
    let s2 = arma_css(y, &phi(sar), &[ma], ic, ncond);
    (ma, sar, ic, s2)
}

fn css_arma11_sar1(y: &[f64], period: usize) -> (f64, f64, f64, f64, f64) {
    let n = y.len();
    let ncond = 1 + period;
    if period < 2 || n <= ncond {
        return (0.0, 0.0, 0.0, 0.0, f64::NAN);
    }
    let mut ar = 0.0;
    let mut ma = 0.0;
    let mut sar = 0.0;
    let mut ic = y.iter().sum::<f64>() / n as f64;
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    let expand = |ar: f64, sar: f64| -> Vec<f64> {
        let mut phi = vec![0.0; period + 1];
        phi[0] = ar;
        phi[period - 1] = sar;
        phi[period] = -ar * sar;
        phi
    };
    for _ in 0..25 {
        ar = golden_min(-0.99, 0.99, 80, |a| {
            arma_css(y, &expand(a, sar), &[ma], ic, ncond)
        });
        ma = golden_min(-0.99, 0.99, 80, |t| {
            arma_css(y, &expand(ar, sar), &[t], ic, ncond)
        });
        sar = golden_min(-0.99, 0.99, 80, |s| {
            arma_css(y, &expand(ar, s), &[ma], ic, ncond)
        });
        ic = golden_min(lo_ic, hi_ic, 80, |m| {
            arma_css(y, &expand(ar, sar), &[ma], m, ncond)
        });
    }
    let s2 = arma_css(y, &expand(ar, sar), &[ma], ic, ncond);
    (ar, ma, sar, ic, s2)
}

fn css_arma11_sma1(y: &[f64], period: usize) -> (f64, f64, f64, f64, f64) {
    let n = y.len();
    let ncond = 1usize;
    if period < 2 || n <= period + 1 {
        return (0.0, 0.0, 0.0, 0.0, f64::NAN);
    }
    let mut ar = 0.0;
    let mut ma = 0.0;
    let mut sma = 0.0;
    let mut ic = y.iter().sum::<f64>() / n as f64;
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    let expand = |ma: f64, sma: f64| -> Vec<f64> {
        let mut theta = vec![0.0; period + 1];
        theta[0] = ma;
        theta[period - 1] = sma;
        theta[period] = ma * sma;
        theta
    };
    for _ in 0..25 {
        ar = golden_min(-0.99, 0.99, 80, |a| {
            arma_css(y, &[a], &expand(ma, sma), ic, ncond)
        });
        ma = golden_min(-0.99, 0.99, 80, |t| {
            arma_css(y, &[ar], &expand(t, sma), ic, ncond)
        });
        sma = golden_min(-0.99, 0.99, 80, |s| {
            arma_css(y, &[ar], &expand(ma, s), ic, ncond)
        });
        ic = golden_min(lo_ic, hi_ic, 80, |m| {
            arma_css(y, &[ar], &expand(ma, sma), m, ncond)
        });
    }
    let s2 = arma_css(y, &[ar], &expand(ma, sma), ic, ncond);
    (ar, ma, sma, ic, s2)
}

fn css_sar1_sma1(y: &[f64], period: usize) -> (f64, f64, f64, f64) {
    let n = y.len();
    if period < 2 || n <= period {
        return (0.0, 0.0, 0.0, f64::NAN);
    }
    let mut sar = 0.0;
    let mut sma = 0.0;
    let mut ic = y.iter().sum::<f64>() / n as f64;
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    let ncond = period;
    let pad = |v: f64| -> Vec<f64> {
        let mut a = vec![0.0; period];
        a[period - 1] = v;
        a
    };
    for _ in 0..25 {
        sar = golden_min(-0.99, 0.99, 80, |a| {
            arma_css(y, &pad(a), &pad(sma), ic, ncond)
        });
        sma = golden_min(-0.99, 0.99, 80, |t| {
            arma_css(y, &pad(sar), &pad(t), ic, ncond)
        });
        ic = golden_min(lo_ic, hi_ic, 80, |m| {
            arma_css(y, &pad(sar), &pad(sma), m, ncond)
        });
    }
    let s2 = arma_css(y, &pad(sar), &pad(sma), ic, ncond);
    (sar, sma, ic, s2)
}

fn css_ma2(y: &[f64]) -> (f64, f64, f64, f64) {
    let n = y.len();
    if n < 3 {
        return (0.0, 0.0, 0.0, f64::NAN);
    }
    let mut t1 = 0.0;
    let mut t2 = 0.0;
    let mut ic = y.iter().sum::<f64>() / n as f64;
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    for _ in 0..25 {
        t1 = golden_min(-0.99, 0.99, 80, |a| arma_css(y, &[], &[a, t2], ic, 0));
        t2 = golden_min(-0.99, 0.99, 80, |a| arma_css(y, &[], &[t1, a], ic, 0));
        ic = golden_min(lo_ic, hi_ic, 80, |m| arma_css(y, &[], &[t1, t2], m, 0));
    }
    let s2 = arma_css(y, &[], &[t1, t2], ic, 0);
    (t1, t2, ic, s2)
}

fn css_arma11_no_mean(y: &[f64]) -> (f64, f64, f64) {
    let n = y.len();
    if n < 3 {
        return (0.0, 0.0, f64::NAN);
    }
    let mut ar = 0.0;
    let mut ma = 0.0;
    let ncond = 1usize;
    for _ in 0..25 {
        ar = golden_min(-0.99, 0.99, 80, |a| arma_css(y, &[a], &[ma], 0.0, ncond));
        ma = golden_min(-0.99, 0.99, 80, |t| arma_css(y, &[ar], &[t], 0.0, ncond));
    }
    let s2 = arma_css(y, &[ar], &[ma], 0.0, ncond);
    (ar, ma, s2)
}

fn css_ma1_no_mean(y: &[f64]) -> (f64, f64) {
    if y.len() < 2 {
        return (0.0, f64::NAN);
    }
    let th = golden_min(-0.99, 0.99, 80, |t| arma_css(y, &[], &[t], 0.0, 0));
    let s2 = arma_css(y, &[], &[th], 0.0, 0);
    (th, s2)
}

fn css_ma2_no_mean(y: &[f64]) -> (f64, f64, f64) {
    if y.len() < 3 {
        return (0.0, 0.0, f64::NAN);
    }
    let mut t1 = 0.0;
    let mut t2 = 0.0;
    for _ in 0..25 {
        t1 = golden_min(-0.99, 0.99, 80, |a| arma_css(y, &[], &[a, t2], 0.0, 0));
        t2 = golden_min(-0.99, 0.99, 80, |a| arma_css(y, &[], &[t1, a], 0.0, 0));
    }
    let s2 = arma_css(y, &[], &[t1, t2], 0.0, 0);
    (t1, t2, s2)
}

fn css_ma1_sma1_no_mean(y: &[f64], period: usize) -> (f64, f64, f64) {
    if period < 2 || y.len() <= period + 1 {
        return (0.0, 0.0, f64::NAN);
    }
    let mut ma = 0.0;
    let mut sma = 0.0;
    let expand = |ma: f64, sma: f64| -> Vec<f64> {
        let mut theta = vec![0.0; period + 1];
        theta[0] = ma;
        theta[period - 1] = sma;
        theta[period] = ma * sma;
        theta
    };
    for _ in 0..25 {
        ma = golden_min(-0.99, 0.99, 80, |a| {
            arma_css(y, &[], &expand(a, sma), 0.0, 0)
        });
        sma = golden_min(-0.99, 0.99, 80, |s| {
            arma_css(y, &[], &expand(ma, s), 0.0, 0)
        });
    }
    let s2 = arma_css(y, &[], &expand(ma, sma), 0.0, 0);
    (ma, sma, s2)
}

fn difference_series(y: &[f64], d: i32) -> Vec<f64> {
    let mut out = y.to_vec();
    for _ in 0..d.max(0) {
        if out.len() < 2 {
            return out;
        }
        out = out.windows(2).map(|w| w[1] - w[0]).collect();
    }
    out
}

fn seasonal_difference(y: &[f64], period: usize, d: i32) -> Vec<f64> {
    let mut out = y.to_vec();
    if period < 2 {
        return out;
    }
    for _ in 0..d.max(0) {
        if out.len() <= period {
            return out;
        }
        out = (period..out.len())
            .map(|i| out[i] - out[i - period])
            .collect();
    }
    out
}

fn css_ar_no_mean(y: &[f64], p: usize) -> (Vec<f64>, f64) {
    let ncond = p;
    if y.len() <= ncond {
        return (vec![0.0; p], f64::NAN);
    }
    let mut phi = vec![0.0; p];
    for _ in 0..25 {
        for i in 0..p {
            phi[i] = golden_min(-0.99, 0.99, 80, |t| {
                let mut trial = phi.clone();
                trial[i] = t;
                arma_css(y, &trial, &[], 0.0, ncond)
            });
        }
    }
    let s2 = arma_css(y, &phi, &[], 0.0, ncond);
    (phi, s2)
}

fn css_ar_mean(y: &[f64], p: usize) -> (Vec<f64>, f64, f64) {
    let ncond = p;
    if p == 0 || y.len() <= ncond {
        return (vec![0.0; p], 0.0, f64::NAN);
    }
    let mut phi = vec![0.0; p];
    let mut ic = y.iter().sum::<f64>() / y.len() as f64;
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    for _ in 0..25 {
        for i in 0..p {
            phi[i] = golden_min(-0.99, 0.99, 80, |t| {
                let mut trial = phi.clone();
                trial[i] = t;
                arma_css(y, &trial, &[], ic, ncond)
            });
        }
        ic = golden_min(lo_ic, hi_ic, 80, |m| arma_css(y, &phi, &[], m, ncond));
    }
    let s2 = arma_css(y, &phi, &[], ic, ncond);
    (phi, ic, s2)
}

fn css_ar1_sar1_no_mean(y: &[f64], period: usize) -> (f64, f64, f64) {
    let ncond = 1 + period;
    if period < 2 || y.len() <= ncond {
        return (0.0, 0.0, f64::NAN);
    }
    let mut ar = 0.0;
    let mut sar = 0.0;
    let expand = |ar: f64, sar: f64| -> Vec<f64> {
        let mut phi = vec![0.0; period + 1];
        phi[0] = ar;
        phi[period - 1] = sar;
        phi[period] = -ar * sar;
        phi
    };
    for _ in 0..25 {
        ar = golden_min(-0.99, 0.99, 80, |a| {
            arma_css(y, &expand(a, sar), &[], 0.0, ncond)
        });
        sar = golden_min(-0.99, 0.99, 80, |s| {
            arma_css(y, &expand(ar, s), &[], 0.0, ncond)
        });
    }
    let s2 = arma_css(y, &expand(ar, sar), &[], 0.0, ncond);
    (ar, sar, s2)
}

fn css_arma11_sar1_no_mean(y: &[f64], period: usize) -> (f64, f64, f64, f64) {
    let ncond = 1 + period;
    if period < 2 || y.len() <= ncond {
        return (0.0, 0.0, 0.0, f64::NAN);
    }
    let mut ar = 0.0;
    let mut ma = 0.0;
    let mut sar = 0.0;
    let expand = |ar: f64, sar: f64| -> Vec<f64> {
        let mut phi = vec![0.0; period + 1];
        phi[0] = ar;
        phi[period - 1] = sar;
        phi[period] = -ar * sar;
        phi
    };
    for _ in 0..25 {
        ar = golden_min(-0.99, 0.99, 80, |a| {
            arma_css(y, &expand(a, sar), &[ma], 0.0, ncond)
        });
        ma = golden_min(-0.99, 0.99, 80, |t| {
            arma_css(y, &expand(ar, sar), &[t], 0.0, ncond)
        });
        sar = golden_min(-0.99, 0.99, 80, |s| {
            arma_css(y, &expand(ar, s), &[ma], 0.0, ncond)
        });
    }
    let s2 = arma_css(y, &expand(ar, sar), &[ma], 0.0, ncond);
    (ar, ma, sar, s2)
}

fn css_arma11_sma1_no_mean(y: &[f64], period: usize) -> (f64, f64, f64, f64) {
    let ncond = 1usize;
    if period < 2 || y.len() <= period + 1 {
        return (0.0, 0.0, 0.0, f64::NAN);
    }
    let mut ar = 0.0;
    let mut ma = 0.0;
    let mut sma = 0.0;
    let expand = |ma: f64, sma: f64| -> Vec<f64> {
        let mut theta = vec![0.0; period + 1];
        theta[0] = ma;
        theta[period - 1] = sma;
        theta[period] = ma * sma;
        theta
    };
    for _ in 0..25 {
        ar = golden_min(-0.99, 0.99, 80, |a| {
            arma_css(y, &[a], &expand(ma, sma), 0.0, ncond)
        });
        ma = golden_min(-0.99, 0.99, 80, |t| {
            arma_css(y, &[ar], &expand(t, sma), 0.0, ncond)
        });
        sma = golden_min(-0.99, 0.99, 80, |s| {
            arma_css(y, &[ar], &expand(ma, s), 0.0, ncond)
        });
    }
    let s2 = arma_css(y, &[ar], &expand(ma, sma), 0.0, ncond);
    (ar, ma, sma, s2)
}

fn css_arma11_sar1_sma1(y: &[f64], period: usize) -> (f64, f64, f64, f64, f64, f64) {
    let n = y.len();
    let ncond = 1 + period;
    if period < 2 || n <= ncond {
        return (0.0, 0.0, 0.0, 0.0, 0.0, f64::NAN);
    }
    let mut ar = 0.0;
    let mut ma = 0.0;
    let mut sar = 0.0;
    let mut sma = 0.0;
    let mut ic = y.iter().sum::<f64>() / n as f64;
    let lo_ic = y.iter().copied().fold(f64::INFINITY, f64::min) - 1.0;
    let hi_ic = y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1.0;
    let ephi = |ar: f64, sar: f64| -> Vec<f64> {
        let mut phi = vec![0.0; period + 1];
        phi[0] = ar;
        phi[period - 1] = sar;
        phi[period] = -ar * sar;
        phi
    };
    let eth = |ma: f64, sma: f64| -> Vec<f64> {
        let mut theta = vec![0.0; period + 1];
        theta[0] = ma;
        theta[period - 1] = sma;
        theta[period] = ma * sma;
        theta
    };
    for _ in 0..25 {
        ar = golden_min(-0.99, 0.99, 80, |a| {
            arma_css(y, &ephi(a, sar), &eth(ma, sma), ic, ncond)
        });
        ma = golden_min(-0.99, 0.99, 80, |t| {
            arma_css(y, &ephi(ar, sar), &eth(t, sma), ic, ncond)
        });
        sar = golden_min(-0.99, 0.99, 80, |s| {
            arma_css(y, &ephi(ar, s), &eth(ma, sma), ic, ncond)
        });
        sma = golden_min(-0.99, 0.99, 80, |s| {
            arma_css(y, &ephi(ar, sar), &eth(ma, s), ic, ncond)
        });
        ic = golden_min(lo_ic, hi_ic, 80, |m| {
            arma_css(y, &ephi(ar, sar), &eth(ma, sma), m, ncond)
        });
    }
    let s2 = arma_css(y, &ephi(ar, sar), &eth(ma, sma), ic, ncond);
    (ar, ma, sar, sma, ic, s2)
}

fn css_arma11_sar1_sma1_no_mean(y: &[f64], period: usize) -> (f64, f64, f64, f64, f64) {
    let ncond = 1 + period;
    if period < 2 || y.len() <= ncond {
        return (0.0, 0.0, 0.0, 0.0, f64::NAN);
    }
    let mut ar = 0.0;
    let mut ma = 0.0;
    let mut sar = 0.0;
    let mut sma = 0.0;
    let ephi = |ar: f64, sar: f64| -> Vec<f64> {
        let mut phi = vec![0.0; period + 1];
        phi[0] = ar;
        phi[period - 1] = sar;
        phi[period] = -ar * sar;
        phi
    };
    let eth = |ma: f64, sma: f64| -> Vec<f64> {
        let mut theta = vec![0.0; period + 1];
        theta[0] = ma;
        theta[period - 1] = sma;
        theta[period] = ma * sma;
        theta
    };
    for _ in 0..25 {
        ar = golden_min(-0.99, 0.99, 80, |a| {
            arma_css(y, &ephi(a, sar), &eth(ma, sma), 0.0, ncond)
        });
        ma = golden_min(-0.99, 0.99, 80, |t| {
            arma_css(y, &ephi(ar, sar), &eth(t, sma), 0.0, ncond)
        });
        sar = golden_min(-0.99, 0.99, 80, |s| {
            arma_css(y, &ephi(ar, s), &eth(ma, sma), 0.0, ncond)
        });
        sma = golden_min(-0.99, 0.99, 80, |s| {
            arma_css(y, &ephi(ar, sar), &eth(ma, s), 0.0, ncond)
        });
    }
    let s2 = arma_css(y, &ephi(ar, sar), &eth(ma, sma), 0.0, ncond);
    (ar, ma, sar, sma, s2)
}

/// GNU `arima` — AR(0..2)/MA(1), CSS or ML, optional difference.
pub unsafe fn do_arima(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x) as usize;
        if n < 1 {
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
        let mut css = false;
        let mut p = 1i32;
        let mut d = 0i32;
        let mut q = 0i32;
        let mut sar_p = 0i32;
        let mut sar_d = 0i32;
        let mut sar_q = 0i32;
        let mut period = 0i32;
        let mut include_mean = true;
        let mut a = CDR(args);
        while !a.is_null() && a != R_NilValue() {
            let tag = TAG(a);
            if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag))).to_string_lossy();
                let v = CAR(a);
                if name == "method" {
                    if !v.is_null() && TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                        let m = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(v, 0))).to_string_lossy();
                        css = m == "CSS";
                    }
                } else if name == "order" && !v.is_null() && XLENGTH(v) >= 3 {
                    let elt = |i: i64| {
                        if TYPEOF(v) == SEXPTYPE::INTSXP {
                            *INTEGER(v).add(i as usize) as i32
                        } else {
                            *REAL(v).add(i as usize) as i32
                        }
                    };
                    p = elt(0);
                    d = elt(1);
                    q = elt(2);
                } else if name == "seasonal" && !v.is_null() && TYPEOF(v) == SEXPTYPE::VECSXP {
                    let ord = named_list_elt(v, "order");
                    let per = named_list_elt(v, "period");
                    let ival = |x: SEXP, i: usize| -> i32 {
                        if x.is_null() || x == R_NilValue() || XLENGTH(x) <= i as i64 {
                            0
                        } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                            *INTEGER(x).add(i)
                        } else if TYPEOF(x) == SEXPTYPE::REALSXP {
                            *REAL(x).add(i) as i32
                        } else {
                            0
                        }
                    };
                    sar_p = ival(ord, 0);
                    sar_d = ival(ord, 1);
                    sar_q = ival(ord, 2);
                    period = ival(per, 0);
                } else if name == "include.mean" {
                    include_mean = asBool(v);
                }
            }
            a = CDR(a);
        }
        if d > 0 {
            y = difference_series(&y, d);
        }
        if sar_d > 0 && period >= 2 {
            y = seasonal_difference(&y, period as usize, sar_d);
        }
        let differenced = d > 0 || sar_d > 0;
        let no_mean = differenced || !include_mean;
        let (values, names, sigma2): (Vec<f64>, Vec<String>, f64) = if p <= 0
            && q <= 0
            && !no_mean
            && !css
            && sar_p == 1
            && sar_q == 1
            && period >= 2
        {
            exact_arima_ml(&y, 0, 0, 1, 1, period as usize, true)
        } else if p <= 0 && q <= 0 && !differenced && sar_p == 1 && sar_q == 1 && period >= 2 {
            let (sar, sma, mu, s2) = css_sar1_sma1(&y, period as usize);
            (
                vec![sar, sma, mu],
                vec![
                    "sar1".to_string(),
                    "sma1".to_string(),
                    "intercept".to_string(),
                ],
                s2,
            )
        } else if p <= 0 && q <= 0 && !no_mean && !css && sar_p == 1 && sar_q <= 0 && period >= 2 {
            exact_arima_ml(&y, 0, 0, 1, 0, period as usize, true)
        } else if p <= 0 && q <= 0 && !differenced && sar_p == 1 && sar_q <= 0 && period >= 2 {
            let (sar, mu, s2) = css_sar1(&y, period as usize);
            (
                vec![sar, mu],
                vec!["sar1".to_string(), "intercept".to_string()],
                s2,
            )
        } else if p <= 0 && q <= 0 && !no_mean && !css && sar_p <= 0 && sar_q == 1 && period >= 2 {
            exact_arima_ml(&y, 0, 0, 0, 1, period as usize, true)
        } else if p <= 0 && q <= 0 && !differenced && sar_p <= 0 && sar_q == 1 && period >= 2 {
            let (sma, mu, s2) = css_sma1(&y, period as usize);
            (
                vec![sma, mu],
                vec!["sma1".to_string(), "intercept".to_string()],
                s2,
            )
        } else if p == 1 && q <= 0 && !no_mean && !css && sar_p == 1 && sar_q <= 0 && period >= 2 {
            exact_arima_ml(&y, 1, 0, 1, 0, period as usize, true)
        } else if p == 1 && q <= 0 && !differenced && sar_p == 1 && period >= 2 {
            let (ar, sar, mu, s2) = css_ar1_sar1(&y, period as usize);
            (
                vec![ar, sar, mu],
                vec![
                    "ar1".to_string(),
                    "sar1".to_string(),
                    "intercept".to_string(),
                ],
                s2,
            )
        } else if p <= 0 && q == 1 && !no_mean && !css && sar_p <= 0 && sar_q == 1 && period >= 2 {
            exact_arima_ml(&y, 0, 1, 0, 1, period as usize, true)
        } else if p <= 0 && q == 1 && !differenced && sar_q == 1 && period >= 2 {
            let (ma, sma, mu, s2) = css_ma1_sma1(&y, period as usize);
            (
                vec![ma, sma, mu],
                vec![
                    "ma1".to_string(),
                    "sma1".to_string(),
                    "intercept".to_string(),
                ],
                s2,
            )
        } else if p == 1 && q <= 0 && !no_mean && !css && sar_p <= 0 && sar_q == 1 && period >= 2 {
            exact_arima_ml(&y, 1, 0, 0, 1, period as usize, true)
        } else if p == 1 && q <= 0 && !differenced && sar_q == 1 && period >= 2 {
            let (ar, sma, mu, s2) = css_ar1_sma1(&y, period as usize);
            (
                vec![ar, sma, mu],
                vec![
                    "ar1".to_string(),
                    "sma1".to_string(),
                    "intercept".to_string(),
                ],
                s2,
            )
        } else if p <= 0 && q == 1 && !no_mean && !css && sar_p == 1 && sar_q <= 0 && period >= 2 {
            exact_arima_ml(&y, 0, 1, 1, 0, period as usize, true)
        } else if p <= 0 && q == 1 && !differenced && sar_p == 1 && period >= 2 {
            let (ma, sar, mu, s2) = css_ma1_sar1(&y, period as usize);
            (
                vec![ma, sar, mu],
                vec![
                    "ma1".to_string(),
                    "sar1".to_string(),
                    "intercept".to_string(),
                ],
                s2,
            )
        } else if p <= 0 && q <= 0 {
            if differenced {
                let s2 = if y.is_empty() {
                    f64::NAN
                } else {
                    y.iter().map(|v| v * v).sum::<f64>() / y.len() as f64
                };
                (vec![], vec![], s2)
            } else {
                let (mu, s2) = css_ar0(&y);
                (vec![mu], vec!["intercept".to_string()], s2)
            }
        } else if p <= 0 && q == 1 && !no_mean && !css {
            exact_arima_ml(&y, 0, 1, 0, 0, 0, true)
        } else if p <= 0 && q == 1 && d <= 0 && !no_mean {
            let (th, mu, s2) = css_ma1(&y);
            (
                vec![th, mu],
                vec!["ma1".to_string(), "intercept".to_string()],
                s2,
            )
        } else if p <= 0 && q == 2 && !no_mean && !css && sar_p <= 0 && sar_q <= 0 {
            exact_arima_ml(&y, 0, 2, 0, 0, 0, true)
        } else if p <= 0 && q == 2 && !differenced && !no_mean {
            let (t1, t2, mu, s2) = css_ma2(&y);
            (
                vec![t1, t2, mu],
                vec![
                    "ma1".to_string(),
                    "ma2".to_string(),
                    "intercept".to_string(),
                ],
                s2,
            )
        } else if p == 1 && q == 1 && !no_mean && !css && sar_p == 1 && sar_q == 1 && period >= 2 {
            exact_arima_ml(&y, 1, 1, 1, 1, period as usize, true)
        } else if p == 1 && q == 1 && !differenced && sar_p == 1 && sar_q == 1 && period >= 2 {
            let (ar, ma, sar, sma, mu, s2) = css_arma11_sar1_sma1(&y, period as usize);
            (
                vec![ar, ma, sar, sma, mu],
                vec![
                    "ar1".to_string(),
                    "ma1".to_string(),
                    "sar1".to_string(),
                    "sma1".to_string(),
                    "intercept".to_string(),
                ],
                s2,
            )
        } else if p == 1 && q == 1 && !no_mean && !css && sar_p == 1 && sar_q <= 0 && period >= 2 {
            exact_arima_ml(&y, 1, 1, 1, 0, period as usize, true)
        } else if p == 1 && q == 1 && !differenced && sar_p == 1 && sar_q <= 0 && period >= 2 {
            let (ar, ma, sar, mu, s2) = css_arma11_sar1(&y, period as usize);
            (
                vec![ar, ma, sar, mu],
                vec![
                    "ar1".to_string(),
                    "ma1".to_string(),
                    "sar1".to_string(),
                    "intercept".to_string(),
                ],
                s2,
            )
        } else if p == 1 && q == 1 && !no_mean && !css && sar_p <= 0 && sar_q == 1 && period >= 2 {
            exact_arima_ml(&y, 1, 1, 0, 1, period as usize, true)
        } else if p == 1 && q == 1 && !differenced && sar_q == 1 && sar_p <= 0 && period >= 2 {
            let (ar, ma, sma, mu, s2) = css_arma11_sma1(&y, period as usize);
            (
                vec![ar, ma, sma, mu],
                vec![
                    "ar1".to_string(),
                    "ma1".to_string(),
                    "sma1".to_string(),
                    "intercept".to_string(),
                ],
                s2,
            )
        } else if p == 1 && q == 1 && !no_mean && !css && sar_p <= 0 && sar_q <= 0 {
            exact_arima_ml(&y, 1, 1, 0, 0, 0, true)
        } else if p == 1 && q == 1 && d <= 0 && !no_mean {
            let (phi, th, mu, s2) = css_arma11(&y);
            (
                vec![phi, th, mu],
                vec![
                    "ar1".to_string(),
                    "ma1".to_string(),
                    "intercept".to_string(),
                ],
                s2,
            )
        } else if p == 2 && q <= 0 && !no_mean && !css && sar_p <= 0 && sar_q <= 0 {
            exact_arima_ml(&y, 2, 0, 0, 0, 0, true)
        } else if p == 3 && q <= 0 && !no_mean && !css && sar_p <= 0 && sar_q <= 0 {
            exact_arima_ml(&y, 3, 0, 0, 0, 0, true)
        } else if !css
            && !no_mean
            && p >= 1
            && p <= 3
            && q >= 1
            && q <= 2
            && sar_p <= 0
            && sar_q <= 0
        {
            exact_arima_ml(&y, p as usize, q as usize, 0, 0, 0, true)
        } else if p >= 2 && p <= 5 && q <= 0 && !no_mean && sar_p <= 0 && sar_q <= 0 {
            let (phi, mu, s2) = css_ar_mean(&y, p as usize);
            let mut names: Vec<String> = (1..=p).map(|i| format!("ar{i}")).collect();
            names.push("intercept".to_string());
            let mut values = phi;
            values.push(mu);
            (values, names, s2)
        } else if no_mean && p == 1 && q == 1 && sar_p == 1 && sar_q == 1 && period >= 2 {
            let (ar, ma, sar, sma, s2) = css_arma11_sar1_sma1_no_mean(&y, period as usize);
            (
                vec![ar, ma, sar, sma],
                vec![
                    "ar1".to_string(),
                    "ma1".to_string(),
                    "sar1".to_string(),
                    "sma1".to_string(),
                ],
                s2,
            )
        } else if no_mean && p == 1 && q == 1 && sar_p == 1 && sar_q <= 0 && period >= 2 {
            let (ar, ma, sar, s2) = css_arma11_sar1_no_mean(&y, period as usize);
            (
                vec![ar, ma, sar],
                vec!["ar1".to_string(), "ma1".to_string(), "sar1".to_string()],
                s2,
            )
        } else if no_mean && p == 1 && q == 1 && sar_q == 1 && sar_p <= 0 && period >= 2 {
            let (ar, ma, sma, s2) = css_arma11_sma1_no_mean(&y, period as usize);
            (
                vec![ar, ma, sma],
                vec!["ar1".to_string(), "ma1".to_string(), "sma1".to_string()],
                s2,
            )
        } else if !css
            && no_mean
            && p >= 1
            && p <= 3
            && q >= 1
            && q <= 2
            && sar_p <= 0
            && sar_q <= 0
        {
            exact_arima_ml(&y, p as usize, q as usize, 0, 0, 0, false)
        } else if no_mean && p == 1 && q == 1 {
            let (ar, ma, s2) = css_arma11_no_mean(&y);
            (vec![ar, ma], vec!["ar1".to_string(), "ma1".to_string()], s2)
        } else if !css && no_mean && p <= 0 && q == 1 && sar_p <= 0 && sar_q == 1 && period >= 2 {
            exact_arima_ml(&y, 0, 1, 0, 1, period as usize, false)
        } else if no_mean && p <= 0 && q == 1 && sar_q == 1 && period >= 2 {
            let (ma, sma, s2) = css_ma1_sma1_no_mean(&y, period as usize);
            (
                vec![ma, sma],
                vec!["ma1".to_string(), "sma1".to_string()],
                s2,
            )
        } else if !css && no_mean && p <= 0 && q == 1 && sar_p <= 0 && sar_q <= 0 {
            exact_arima_ml(&y, 0, 1, 0, 0, 0, false)
        } else if no_mean && p <= 0 && q == 1 {
            let (th, s2) = css_ma1_no_mean(&y);
            (vec![th], vec!["ma1".to_string()], s2)
        } else if no_mean && p <= 0 && q == 2 {
            let (t1, t2, s2) = css_ma2_no_mean(&y);
            (vec![t1, t2], vec!["ma1".to_string(), "ma2".to_string()], s2)
        } else if no_mean && p == 1 && q <= 0 && sar_p == 1 && period >= 2 {
            let (ar, sar, s2) = css_ar1_sar1_no_mean(&y, period as usize);
            (
                vec![ar, sar],
                vec!["ar1".to_string(), "sar1".to_string()],
                s2,
            )
        } else if !css && no_mean && p == 1 && q <= 0 && sar_p <= 0 && sar_q <= 0 {
            exact_arima_ml(&y, 1, 0, 0, 0, 0, false)
        } else if !css && no_mean && p >= 2 && p <= 3 && q <= 0 && sar_p <= 0 && sar_q <= 0 {
            exact_arima_ml(&y, p as usize, 0, 0, 0, 0, false)
        } else if no_mean && q <= 0 && p > 0 {
            let (phi, s2) = css_ar_no_mean(&y, p as usize);
            let names: Vec<String> = (1..=p).map(|i| format!("ar{i}")).collect();
            (phi, names, s2)
        } else {
            let (phi, mu, s2) = if css { css_ar1(&y) } else { exact_ar1_ml(&y) };
            (
                vec![phi, mu],
                vec!["ar1".to_string(), "intercept".to_string()],
                s2,
            )
        };
        let coef = Rf_allocVector3(SEXPTYPE::REALSXP, values.len() as i64);
        let _c = protect(coef);
        for (i, v) in values.iter().enumerate() {
            *REAL(coef).add(i) = *v;
        }
        crate::mainutils::essentials::set_string_names(coef, &names);
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
        unsafe { crate::mainutils::errors::R_getCurrentCall() },
        "'arima0.diag' is defunct.\nSee help(\"Defunct\")",
    );
}

/// GNU `plclust(...)` — `.Defunct("plot")`.
pub unsafe fn do_plclust(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::mainutils::errors::errorcall_str(
        unsafe { crate::mainutils::errors::R_getCurrentCall() },
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
                        *REAL(start_innov)
                            .add(i.min((XLENGTH(start_innov) as usize).saturating_sub(1)))
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
