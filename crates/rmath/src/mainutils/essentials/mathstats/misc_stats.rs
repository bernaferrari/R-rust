//! ppoints, qqnorm, mahalanobis, birthday, lsfit, poly, loglin, smirnov, smooth — extracted verbatim from essentials/mathstats.rs.

use super::*;

/// GNU `ppoints(n, a=)` — probability points.
pub unsafe fn do_ppoints(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if XLENGTH(n_s) > 1 {
            XLENGTH(n_s) as f64
        } else {
            elt_real_safe(n_s, 0)
        };
        if n <= 0.0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let ni = n.round() as i64;
        let a = if ni <= 10 { 3.0 / 8.0 } else { 0.5 };
        let den = n + 1.0 - 2.0 * a;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, ni);
        let _r = protect(result);
        for i in 0..ni {
            *REAL(result).add(i as usize) = ((i as f64 + 1.0) - a) / den;
        }
        result
    }
}

/// GNU `qqnorm(y, plot.it=FALSE)` — normal theoretical quantiles vs sorted y.
pub unsafe fn do_qqnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let y = CAR(args);
        let n = XLENGTH(y) as usize;
        if n == 0 {
            return R_NilValue();
        }
        let mut ys: Vec<f64> = (0..n).map(|i| elt_real_safe(y, i as i64)).collect();
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let a = if n <= 10 { 3.0 / 8.0 } else { 0.5 };
        let den = n as f64 + 1.0 - 2.0 * a;
        let xv = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _x = protect(xv);
        let yv = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _y = protect(yv);
        for i in 0..n {
            let p = ((i as f64 + 1.0) - a) / den;
            *REAL(xv).add(i) = crate::dist::normal::qnorm5_inner(p, 0.0, 1.0, true, false);
            *REAL(yv).add(i) = ys[i];
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, xv);
        SET_VECTOR_ELT(result, 1, yv);
        crate::mainutils::essentials::set_string_names(result, &["x".to_string(), "y".to_string()]);
        result
    }
}

/// GNU `qqline(y)` — type-7 quartile line then `abline`.
pub unsafe fn do_qqline(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let y = CAR(args);
        let n = if y.is_null() || y == R_NilValue() {
            0
        } else {
            XLENGTH(y)
        };
        let mut vals = Vec::new();
        for i in 0..n {
            let v = elt_real_safe(y, i);
            if v.is_finite() {
                vals.push(v);
            }
        }
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let y1 = quantile_type7(&vals, 0.25);
        let y2 = quantile_type7(&vals, 0.75);
        let x1 = crate::dist::normal::qnorm5_inner(0.25, 0.0, 1.0, true, false);
        let x2 = crate::dist::normal::qnorm5_inner(0.75, 0.0, 1.0, true, false);
        let slope = (y2 - y1) / (x2 - x1);
        let int = y1 - slope * x1;

        let ab = crate::sexp::constructors::Rf_lang3(
            Rf_install(c"abline".as_ptr()),
            Rf_ScalarReal(int),
            Rf_ScalarReal(slope),
        );
        let _ab = protect(ab);
        crate::eval::eval::Rf_eval(ab, rho)
    }
}

/// GNU `mahalanobis(x, center, cov)` — squared Mahalanobis, 2 columns.
pub unsafe fn do_mahalanobis(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let center = CAR(CDR(args));
        let cov = CAR(CDR(CDR(args)));
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (n, p) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            return R_NilValue();
        };
        if p != 2 || n == 0 {
            return R_NilValue();
        }
        let c0 = elt_real_safe(center, 0);
        let c1 = elt_real_safe(center, 1);
        let a00 = elt_real_safe(cov, 0);
        let a10 = elt_real_safe(cov, 1);
        let a01 = if XLENGTH(cov) > 2 {
            elt_real_safe(cov, 2)
        } else {
            a10
        };
        let a11 = if XLENGTH(cov) > 3 {
            elt_real_safe(cov, 3)
        } else {
            a00
        };
        let Some(inv) = invert2(a00, a10, a01, a11) else {
            return R_NilValue();
        };
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _r = protect(result);
        for i in 0..n {
            let d0 = elt_real_safe(x, i as i64) - c0;
            let d1 = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i + n)
            } else {
                *INTEGER(x).add(i + n) as f64
            } - c1;
            let t0 = inv.0 * d0 + inv.2 * d1;
            let t1 = inv.1 * d0 + inv.3 * d1;
            *REAL(result).add(i) = d0 * t0 + d1 * t1;
        }
        result
    }
}

/// GNU `cov.wt(x)` — unbiased 2-column covariance and center.
pub unsafe fn do_cov_wt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (n, p) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            return R_NilValue();
        };
        if p != 2 || n < 2 {
            return R_NilValue();
        }
        let mut m0 = 0.0;
        let mut m1 = 0.0;
        for i in 0..n {
            m0 += elt_real_safe(x, i as i64);
            m1 += if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i + n)
            } else {
                *INTEGER(x).add(i + n) as f64
            };
        }
        let nf = n as f64;
        m0 /= nf;
        m1 /= nf;
        let mut s00 = 0.0;
        let mut s01 = 0.0;
        let mut s11 = 0.0;
        for i in 0..n {
            let d0 = elt_real_safe(x, i as i64) - m0;
            let d1 = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i + n)
            } else {
                *INTEGER(x).add(i + n) as f64
            } - m1;
            s00 += d0 * d0;
            s01 += d0 * d1;
            s11 += d1 * d1;
        }
        let den = nf - 1.0;
        s00 /= den;
        s01 /= den;
        s11 /= den;
        let cov = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 2, 2);
        let _c = protect(cov);
        *REAL(cov) = s00;
        *REAL(cov).add(1) = s01;
        *REAL(cov).add(2) = s01;
        *REAL(cov).add(3) = s11;
        let center = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _ce = protect(center);
        *REAL(center) = m0;
        *REAL(center).add(1) = m1;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, cov);
        SET_VECTOR_ELT(result, 1, center);
        SET_VECTOR_ELT(result, 2, Rf_ScalarInteger(n as i32));
        crate::mainutils::essentials::set_string_names(
            result,
            &["cov".to_string(), "center".to_string(), "n.obs".to_string()],
        );
        result
    }
}

/// GNU `cov2cor(V)` — scale a 2×2 covariance to a correlation.
pub unsafe fn do_cov2cor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let v = CAR(args);
        if XLENGTH(v) < 4 {
            return R_NilValue();
        }
        let a00 = elt_real_safe(v, 0);
        let a10 = elt_real_safe(v, 1);
        let a01 = elt_real_safe(v, 2);
        let a11 = elt_real_safe(v, 3);
        let s0 = a00.max(0.0).sqrt();
        let s1 = a11.max(0.0).sqrt();
        let result = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 2, 2);
        let _r = protect(result);
        *REAL(result) = 1.0;
        let c01 = if s0 > 0.0 && s1 > 0.0 {
            a01 / (s0 * s1)
        } else {
            0.0
        };
        let c10 = if s0 > 0.0 && s1 > 0.0 {
            a10 / (s0 * s1)
        } else {
            0.0
        };
        *REAL(result).add(1) = c10;
        *REAL(result).add(2) = c01;
        *REAL(result).add(3) = 1.0;
        result
    }
}

/// GNU `pbirthday(n, classes=365, coincident=2)` — collision probability.
pub unsafe fn do_pbirthday(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let n = if TYPEOF(n_arg) == SEXPTYPE::INTSXP {
            *INTEGER(n_arg) as i64
        } else {
            elt_real_safe(n_arg, 0) as i64
        };
        let classes_cell = CDR(args);
        let classes = if classes_cell.is_null()
            || classes_cell == R_NilValue()
            || CAR(classes_cell) == R_MissingArg()
        {
            365.0
        } else {
            elt_real_safe(CAR(classes_cell), 0)
        };
        if n <= 0 {
            return Rf_ScalarReal(0.0);
        }
        if classes <= 0.0 {
            return Rf_ScalarReal(1.0);
        }
        let mut prod = 1.0;
        for i in 0..n {
            prod *= (classes - i as f64) / classes;
        }
        Rf_ScalarReal(1.0 - prod)
    }
}

fn pbirthday_k2(n: i64, classes: f64) -> f64 {
    if n <= 0 {
        return 0.0;
    }
    if classes <= 0.0 {
        return 1.0;
    }
    let mut prod = 1.0;
    for i in 0..n {
        prod *= (classes - i as f64) / classes;
    }
    1.0 - prod
}

/// GNU `qbirthday(prob=0.5, classes=365, coincident=2)` — smallest n.
pub unsafe fn do_qbirthday(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let p_arg = CAR(args);
        let p = if p_arg.is_null() || p_arg == R_NilValue() || p_arg == R_MissingArg() {
            0.5
        } else {
            elt_real_safe(p_arg, 0)
        };
        let classes_cell = CDR(args);
        let classes = if classes_cell.is_null()
            || classes_cell == R_NilValue()
            || CAR(classes_cell) == R_MissingArg()
        {
            365.0
        } else {
            elt_real_safe(CAR(classes_cell), 0)
        };
        if p <= 0.0 {
            return Rf_ScalarInteger(1);
        }
        let max_n = classes.ceil() as i64 + 1;
        if p >= 1.0 {
            return Rf_ScalarInteger(max_n as i32);
        }
        let mut n = 1i64;
        while n < max_n && pbirthday_k2(n, classes) < p {
            n += 1;
        }
        Rf_ScalarInteger(n as i32)
    }
}

/// GNU `lsfit(x, y)` — intercept + slope for a single predictor.
pub unsafe fn do_lsfit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let n = XLENGTH(x).min(XLENGTH(y)) as usize;
        if n < 2 {
            return R_NilValue();
        }
        let mut sx = 0.0;
        let mut sy = 0.0;
        for i in 0..n {
            sx += elt_real_safe(x, i as i64);
            sy += elt_real_safe(y, i as i64);
        }
        let nf = n as f64;
        let mx = sx / nf;
        let my = sy / nf;
        let mut num = 0.0;
        let mut den = 0.0;
        for i in 0..n {
            let dx = elt_real_safe(x, i as i64) - mx;
            let dy = elt_real_safe(y, i as i64) - my;
            num += dx * dy;
            den += dx * dx;
        }
        let b = if den > 0.0 { num / den } else { 0.0 };
        let a = my - b * mx;
        let coef = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _c = protect(coef);
        *REAL(coef) = a;
        *REAL(coef).add(1) = b;
        crate::mainutils::essentials::set_string_names(
            coef,
            &["Intercept".to_string(), "X".to_string()],
        );
        let resid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _rs = protect(resid);
        let hat = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _h = protect(hat);
        let invn = 1.0 / nf;
        for i in 0..n {
            let xi = elt_real_safe(x, i as i64);
            let yi = elt_real_safe(y, i as i64);
            *REAL(resid).add(i) = yi - (a + b * xi);
            *REAL(hat).add(i) = if den > 0.0 {
                invn + (xi - mx) * (xi - mx) / den
            } else {
                invn
            };
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, coef);
        SET_VECTOR_ELT(result, 1, resid);
        SET_VECTOR_ELT(result, 2, hat);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "coefficients".to_string(),
                "residuals".to_string(),
                "hat".to_string(),
            ],
        );
        result
    }
}

/// GNU `ppr(x, y, nterms=1)` — one-term linear projection.
pub unsafe fn do_ppr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let n = XLENGTH(x).min(XLENGTH(y)) as usize;
        if n < 2 {
            return R_NilValue();
        }
        let mut sx = 0.0;
        let mut sy = 0.0;
        for i in 0..n {
            sx += elt_real_safe(x, i as i64);
            sy += elt_real_safe(y, i as i64);
        }
        let nf = n as f64;
        let mx = sx / nf;
        let my = sy / nf;
        let mut num = 0.0;
        let mut den = 0.0;
        for i in 0..n {
            let dx = elt_real_safe(x, i as i64) - mx;
            let dy = elt_real_safe(y, i as i64) - my;
            num += dx * dy;
            den += dx * dx;
        }
        let b = if den > 0.0 { num / den } else { 0.0 };
        let a = my - b * mx;
        let fitted = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _f = protect(fitted);
        let mut sse = 0.0;
        for i in 0..n {
            let fi = a + b * elt_real_safe(x, i as i64);
            *REAL(fitted).add(i) = fi;
            let e = elt_real_safe(y, i as i64) - fi;
            sse += e * e;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, fitted);
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(sse));
        crate::mainutils::essentials::set_string_names(
            result,
            &["fitted.values".to_string(), "gofn".to_string()],
        );
        result
    }
}

/// GNU `ls.diag(ls.out)` — leverages from `$hat` or intercept + `X`.
pub unsafe fn do_ls_diag(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let h = list_named_elt(obj, "hat");
        if h == R_NilValue() {
            return R_NilValue();
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, h);
        crate::mainutils::essentials::set_string_names(result, &["hat".to_string()]);
        result
    }
}

/// GNU `ls.print(ls.out, print.it=FALSE)` — coefficient estimates.
pub unsafe fn do_ls_print(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let coef = list_named_elt(obj, "coefficients");
        if coef == R_NilValue() {
            return R_NilValue();
        }
        let p = XLENGTH(coef);
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), p as i32, 1);
        let _m = protect(mat);
        for i in 0..p as usize {
            *REAL(mat).add(i) = elt_real_safe(coef, i as i64);
        }
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _cn = protect(cn);
        SET_STRING_ELT(cn, 0, Rf_mkChar(c"Estimate".as_ptr()));
        let rn =
            crate::sexp::attrib_core::getAttrib(coef, crate::sexp::attrib_core::R_NamesSymbol());
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(mat, crate::sexp::attrib_core::R_DimNamesSymbol(), dn);
        let tables = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _tb = protect(tables);
        SET_VECTOR_ELT(tables, 0, mat);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, tables);
        crate::mainutils::essentials::set_string_names(result, &["coef.table".to_string()]);
        result
    }
}

/// GNU `weighted.mean(x, w)` — `sum(x*w)/sum(w)`.
pub unsafe fn do_weighted_mean(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let w = CAR(CDR(args));
        let n = XLENGTH(x).min(XLENGTH(w)) as usize;
        if n == 0 {
            return Rf_ScalarReal(f64::NAN);
        }
        let mut num = 0.0;
        let mut den = 0.0;
        for i in 0..n {
            let xi = elt_real_safe(x, i as i64);
            let wi = elt_real_safe(w, i as i64);
            num += xi * wi;
            den += wi;
        }
        Rf_ScalarReal(if den != 0.0 { num / den } else { f64::NAN })
    }
}

/// GNU `poly(x, 1, simple=TRUE)` — centered unit-norm linear term.
pub unsafe fn do_poly(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x) as usize;
        if n < 2 {
            return R_NilValue();
        }
        let mut mean = 0.0;
        for i in 0..n {
            mean += elt_real_safe(x, i as i64);
        }
        mean /= n as f64;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _r = protect(result);
        let mut ss = 0.0;
        for i in 0..n {
            let v = elt_real_safe(x, i as i64) - mean;
            *REAL(result).add(i) = v;
            ss += v * v;
        }
        let s = ss.sqrt();
        if s > 0.0 {
            for i in 0..n {
                *REAL(result).add(i) /= s;
            }
        }
        result
    }
}

/// GNU `loglin(table, margin, start, fit, eps, iter, param, print)` — port of
/// r-source/src/library/stats/R/loglin.R, calling the AS 51 kernel
/// (crates/rmath/src/library/stats/loglin.rs, from loglin.c).
pub unsafe fn do_loglin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let missing = |v: SEXP| v.is_null() || v == R_NilValue() || v == R_MissingArg();
        let sexp_real = |v: SEXP| -> f64 {
            let t = TYPEOF(v);
            if t == SEXPTYPE::REALSXP {
                REAL_ELT(v, 0)
            } else if t == SEXPTYPE::INTSXP {
                INTEGER_ELT(v, 0) as f64
            } else if t == SEXPTYPE::LGLSXP {
                LOGICAL_ELT(v, 0) as f64
            } else {
                0.0
            }
        };
        let sexp_int = |v: SEXP| -> c_int {
            let t = TYPEOF(v);
            if t == SEXPTYPE::REALSXP {
                REAL_ELT(v, 0) as c_int
            } else if t == SEXPTYPE::INTSXP {
                INTEGER_ELT(v, 0)
            } else if t == SEXPTYPE::LGLSXP {
                LOGICAL_ELT(v, 0)
            } else {
                0
            }
        };
        let sexp_bool = |v: SEXP| -> bool {
            let t = TYPEOF(v);
            if t == SEXPTYPE::LGLSXP {
                LOGICAL_ELT(v, 0) != 0
            } else if t == SEXPTYPE::REALSXP {
                REAL_ELT(v, 0) != 0.0
            } else if t == SEXPTYPE::INTSXP {
                INTEGER_ELT(v, 0) != 0
            } else {
                false
            }
        };

        // Parse arguments: table, margin, start, fit, eps, iter, param, print
        let (mut table, mut margin, mut start) = (R_NilValue(), R_NilValue(), R_NilValue());
        let (mut rfit, mut print) = (false, true);
        let (mut eps, mut iter) = (0.1_f64, 20_i32);
        {
            let mut pos = 0;
            let mut cell = args;
            while !cell.is_null() && cell != R_NilValue() {
                let tag = TAG(cell);
                let name = if !tag.is_null() && tag != R_NilValue() {
                    CStr::from_ptr(CHAR(PRINTNAME(tag)))
                        .to_string_lossy()
                        .into_owned()
                } else {
                    String::new()
                };
                let val = CAR(cell);
                if !missing(val) {
                    let key = if name.is_empty() {
                        match pos {
                            0 => "table",
                            1 => "margin",
                            2 => "start",
                            3 => "fit",
                            4 => "eps",
                            5 => "iter",
                            6 => "param",
                            7 => "print",
                            _ => "",
                        }
                        .to_string()
                    } else {
                        name.clone()
                    };
                    match key.as_str() {
                        "table" => table = val,
                        "margin" => margin = val,
                        "start" => start = val,
                        "fit" => rfit = sexp_bool(val),
                        "eps" => eps = sexp_real(val),
                        "iter" => iter = sexp_int(val),
                        "print" => print = sexp_bool(val),
                        _ => {}
                    }
                    if name.is_empty() {
                        pos += 1;
                    }
                } else if name.is_empty() {
                    pos += 1;
                }
                cell = CDR(cell);
            }
        }
        if missing(table) || missing(margin) {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "argument \"table\" is missing, with no default",
            );
        }

        let dtab =
            crate::sexp::attrib_core::getAttrib(table, crate::sexp::attrib_core::R_DimSymbol());
        let nvar = LENGTH(dtab) as c_int;
        if nvar == 0 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "no variables",
            );
        }
        let ncon = LENGTH(margin) as c_int;
        let conf = crate::mainutils::array::allocMatrix(SEXPTYPE::INTSXP.as_c_int(), nvar, ncon);
        let _conf_g = protect(conf);
        let confp = INTEGER(conf);
        for i in 0..(nvar * ncon) as usize {
            *confp.add(i) = 0;
        }
        let dimv: Vec<c_int> = (0..nvar).map(|j| INTEGER_ELT(dtab, j as i32)).collect();

        let mut nmar: i64 = 0;
        for k in 0..ncon as i64 {
            let tmp = VECTOR_ELT(margin, k);
            let t = TYPEOF(tmp);
            if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP {
                crate::mainutils::errors::errorcall_str(
                    crate::mainutils::errors::R_getCurrentCall(),
                    "'margin' must contain names or numbers corresponding to 'table'",
                );
            }
            let m = LENGTH(tmp);
            let mut ok = true;
            let mut vars = Vec::with_capacity(m as usize);
            for i in 0..m {
                let x = if t == SEXPTYPE::REALSXP {
                    REAL_ELT(tmp, i)
                } else {
                    INTEGER_ELT(tmp, i) as f64
                };
                if x.is_nan() || x <= 0.0 {
                    ok = false;
                    break;
                }
                vars.push(x as c_int);
            }
            if !ok {
                crate::mainutils::errors::errorcall_str(
                    crate::mainutils::errors::R_getCurrentCall(),
                    "'margin' must contain names or numbers corresponding to 'table'",
                );
            }
            for (i, &v) in vars.iter().enumerate() {
                *confp.add((k as usize) * nvar as usize + i) = v;
                nmar += dimv[(v - 1) as usize] as i64;
            }
        }

        let ntab = LENGTH(table);
        let start = if missing(start) {
            let s = Rf_allocVector3(SEXPTYPE::REALSXP, ntab as i64);
            let _s_g = protect(s);
            for i in 0..ntab {
                *REAL(s).add(i as usize) = 1.0;
            }
            s
        } else {
            start
        };
        let _start_g = protect(start);
        if LENGTH(start) != ntab {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'start' and 'table' must be same length",
            );
        }

        let snmar_s = Rf_ScalarInteger(nmar as c_int);
        let _snmar_g = protect(snmar_s);
        let eps_s = Rf_ScalarReal(eps);
        let _eps_g = protect(eps_s);
        let iter_s = Rf_ScalarInteger(iter);
        let _iter_g = protect(iter_s);
        let z =
            crate::library::stats::loglin::LogLin(dtab, conf, table, start, snmar_s, eps_s, iter_s);
        let _z_g = protect(z);
        let zfit = VECTOR_ELT(z, 0);
        let zdev = VECTOR_ELT(z, 1);
        let nlast = INTEGER_ELT(VECTOR_ELT(z, 2), 0);

        if print {
            if nlast > 0 {
                let msg = format!(
                    "{} iterations: deviation {}\n",
                    nlast,
                    crate::sexp::accessors::REAL_ELT(zdev, nlast as i32 - 1)
                );
                crate::sexp::output::capture_stdout(&msg);
            } else {
                let msg = format!("{} iterations\n", nlast);
                crate::sexp::output::capture_stdout(&msg);
            }
        }

        // fit <- z$fit; attributes(fit) <- attributes(table)
        crate::sexp::attrib_core::setAttrib(zfit, crate::sexp::attrib_core::R_DimSymbol(), dtab);
        let dn = crate::sexp::attrib_core::getAttrib(
            table,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
        );
        if !dn.is_null() && dn != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                zfit,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dn,
            );
        }

        let elt_real = |x: SEXP, i: i32| -> f64 {
            let t = TYPEOF(x);
            if t == SEXPTYPE::REALSXP {
                REAL_ELT(x, i)
            } else if t == SEXPTYPE::INTSXP {
                INTEGER_ELT(x, i) as f64
            } else {
                0.0
            }
        };

        // Pearson chi-sq and likelihood ratio statistics
        let mut pearson = 0.0_f64;
        for i in 0..ntab {
            if elt_real(start, i) > 0.0 {
                let obs = elt_real(table, i);
                let exp = elt_real(zfit, i);
                pearson += (obs - exp) * (obs - exp) / exp;
            }
        }
        let mut lrt = 0.0_f64;
        for i in 0..ntab {
            let obs = elt_real(table, i);
            let exp = elt_real(zfit, i);
            if obs * exp > 0.0 {
                lrt += obs * (obs / exp).ln();
            }
        }
        lrt *= 2.0;

        // Degrees of freedom via dyadic subset representation
        let mut seen = std::collections::HashMap::new();
        for k in 0..ncon as i64 {
            let tmp = VECTOR_ELT(margin, k);
            let t = TYPEOF(tmp);
            let m = LENGTH(tmp);
            let vars: Vec<c_int> = (0..m)
                .map(|i| {
                    if t == SEXPTYPE::REALSXP {
                        REAL_ELT(tmp, i) as c_int
                    } else {
                        INTEGER_ELT(tmp, i)
                    }
                })
                .collect();
            for mask in 1usize..(1usize << (m as usize)) {
                let mut key = 0i64;
                let mut val = 1i64;
                for (i, &v) in vars.iter().enumerate() {
                    if mask & (1 << i) != 0 {
                        key += 1i64 << (v - 1);
                        val *= (dimv[(v - 1) as usize] - 1) as i64;
                    }
                }
                seen.insert(key, val);
            }
        }
        let df = ntab as i64 - seen.values().sum::<i64>() - 1;

        let n_out = if rfit { 5 } else { 4 };
        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, n_out as i64);
        let _ans_g = protect(ans);
        SET_VECTOR_ELT(ans, 0, Rf_ScalarReal(lrt));
        SET_VECTOR_ELT(ans, 1, Rf_ScalarReal(pearson));
        SET_VECTOR_ELT(ans, 2, Rf_ScalarReal(df as f64));
        SET_VECTOR_ELT(ans, 3, margin);
        if rfit {
            SET_VECTOR_ELT(ans, 4, zfit);
        }
        let names: Vec<String> = if rfit {
            vec![
                "lrt".into(),
                "pearson".into(),
                "df".into(),
                "margin".into(),
                "fit".into(),
            ]
        } else {
            vec!["lrt".into(), "pearson".into(), "df".into(), "margin".into()]
        };
        crate::mainutils::essentials::functional::set_string_names(ans, &names);
        ans
    }
}

fn pkolmogorov_two_limit(x: f64) -> f64 {
    if !x.is_finite() || x <= 0.0 {
        return 0.0;
    }
    let mut s = 0.0;
    for k in 1..=80 {
        let kf = k as f64;
        let term = (-2.0 * kf * kf * x * x).exp();
        if k % 2 == 1 {
            s += term;
        } else {
            s -= term;
        }
        if term < 1e-16 {
            break;
        }
    }
    (1.0 - 2.0 * s).clamp(0.0, 1.0)
}

/// GNU `psmirnov(q, sizes, exact=FALSE)` — two-sided Kolmogorov limit.
pub unsafe fn do_psmirnov(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let q = elt_real_safe(CAR(args), 0);
        let sizes = CAR(CDR(args));
        let n1 = elt_real_safe(sizes, 0);
        let n2 = if XLENGTH(sizes) > 1 {
            elt_real_safe(sizes, 1)
        } else {
            n1
        };
        let n = if n1 + n2 > 0.0 {
            n1 * n2 / (n1 + n2)
        } else {
            0.0
        };
        Rf_ScalarReal(pkolmogorov_two_limit(n.sqrt() * q))
    }
}

fn smirnov_hit(q: f64, r: f64, s: f64, two: bool) -> bool {
    if two {
        (r - s).abs() >= q
    } else {
        (r - s) >= q
    }
}

fn psmirnov_uniq(q: f64, m: i32, n: i32, two: bool, lower: bool) -> f64 {
    if m < 0 || n < 0 {
        return f64::NAN;
    }
    let md = m as f64;
    let nd = n as f64;
    let mut u = vec![0.0f64; (n as usize) + 1];
    if lower {
        u[0] = 1.0;
        for j in 1..=n as usize {
            u[j] = if smirnov_hit(q, 0.0, j as f64 / nd, two) {
                0.0
            } else {
                u[j - 1]
            };
        }
        for i in 1..=m {
            let w = i as f64 / (i + n) as f64;
            u[0] = if smirnov_hit(q, i as f64 / md, 0.0, two) {
                0.0
            } else {
                w * u[0]
            };
            for j in 1..=n as usize {
                u[j] = if smirnov_hit(q, i as f64 / md, j as f64 / nd, two) {
                    0.0
                } else {
                    w * u[j] + u[j - 1]
                };
            }
        }
    } else {
        u[0] = 0.0;
        for j in 1..=n as usize {
            u[j] = if smirnov_hit(q, 0.0, j as f64 / nd, two) {
                1.0
            } else {
                u[j - 1]
            };
        }
        for i in 1..=m {
            if smirnov_hit(q, i as f64 / md, 0.0, two) {
                u[0] = 1.0;
            }
            for j in 1..=n as usize {
                if smirnov_hit(q, i as f64 / md, j as f64 / nd, two) {
                    u[j] = 1.0;
                } else {
                    let v = i as f64 / (i + j as i32) as f64;
                    let w = j as f64 / (i + j as i32) as f64;
                    u[j] = v * u[j] + w * u[j - 1];
                }
            }
        }
    }
    u[n as usize]
}

pub unsafe extern "C-unwind" fn c_psmirnov_exact(
    sq: SEXP,
    sm: SEXP,
    sn: SEXP,
    sz: SEXP,
    stwo: SEXP,
    slower: SEXP,
) -> SEXP {
    unsafe {
        let _ = sz;
        let m = if TYPEOF(sm) == SEXPTYPE::REALSXP {
            *REAL(sm) as i32
        } else {
            *INTEGER(sm)
        };
        let n = if TYPEOF(sn) == SEXPTYPE::REALSXP {
            *REAL(sn) as i32
        } else {
            *INTEGER(sn)
        };
        let two = if TYPEOF(stwo) == SEXPTYPE::REALSXP {
            *REAL(stwo) as i32
        } else {
            *INTEGER(stwo)
        } != 0;
        let lower = if TYPEOF(slower) == SEXPTYPE::REALSXP {
            *REAL(slower) as i32
        } else {
            *INTEGER(slower)
        } != 0;
        let nq = XLENGTH(sq).max(0);
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, nq);
        let _ans = protect(ans);
        let md = m as f64;
        let nd = n as f64;
        for i in 0..nq as usize {
            let raw = if TYPEOF(sq) == SEXPTYPE::REALSXP {
                *REAL(sq).add(i)
            } else {
                *INTEGER(sq).add(i) as f64
            };
            let q = (0.5 + (raw * md * nd - 1e-7).floor()) / (md * nd);
            *REAL(ans).add(i) = psmirnov_uniq(q, m, n, two, lower);
        }
        ans
    }
}

/// GNU `qsmirnov(p, sizes, exact=FALSE)` — smallest D with psmirnov >= p.
pub unsafe fn do_qsmirnov(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let p = elt_real_safe(CAR(args), 0);
        let sizes = CAR(CDR(args));
        let n1 = elt_real_safe(sizes, 0);
        let n2 = if XLENGTH(sizes) > 1 {
            elt_real_safe(sizes, 1)
        } else {
            n1
        };
        if n1 <= 0.0 || n2 <= 0.0 || !(0.0..=1.0).contains(&p) {
            return Rf_ScalarReal(f64::NAN);
        }
        let n = n1 * n2 / (n1 + n2);
        let steps = n1.max(n2) as i32;
        let mut q = 1.0;
        for k in 0..=steps {
            let cand = k as f64 / steps as f64;
            if pkolmogorov_two_limit(n.sqrt() * cand) >= p {
                q = cand;
                break;
            }
        }
        Rf_ScalarReal(q)
    }
}

/// GNU `rsmirnov(n, sizes, z=NULL, alternative=...)` via `C_Smirnov_sim`.
pub unsafe fn do_rsmirnov(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        if n_arg.is_null() || n_arg == R_NilValue() || XLENGTH(n_arg) == 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let n = elt_real_safe(n_arg, 0).floor() as i32;
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        if n < 0 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid arguments",
            );
        }
        let sizes = CAR(CDR(args));
        if sizes.is_null() || sizes == R_NilValue() || XLENGTH(sizes) != 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "argument 'sizes' must be a vector of length 2",
            );
        }
        let nx = elt_real_safe(sizes, 0).floor() as i32;
        let ny = elt_real_safe(sizes, 1).floor() as i32;
        if nx < 1 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "not enough 'x' data",
            );
        }
        if ny < 1 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "not enough 'y' data",
            );
        }
        let mut z = R_NilValue();
        let mut alternative = "two.sided";
        let mut cell = CDR(CDR(args));
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let val = CAR(cell);
            if name == "alternative" {
                alternative = smirnov_alternative(val);
            } else if name == "z" {
                z = val;
            } else if name.is_empty() {
                if pos == 0 {
                    z = val;
                } else if pos == 1 {
                    alternative = smirnov_alternative(val);
                }
                pos += 1;
            }
            cell = CDR(cell);
        }
        if z == crate::sexp::globals::R_MissingArg() {
            z = R_NilValue();
        }
        let two_sided = alternative == "two.sided";
        let (c0, c1) = if alternative == "less" {
            (ny, nx)
        } else {
            (nx, ny)
        };
        let nrowt: Vec<i32> = if z.is_null() || z == R_NilValue() {
            vec![1; (nx + ny) as usize]
        } else {
            smirnov_row_totals(z)
        };
        if nrowt.is_empty() {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let nrow = nrowt.len() as i32;
        let ncolt = [c0, c1];
        let ntotal = nx + ny;
        let mut fact = vec![0.0_f64; (ntotal + 1) as usize];
        fact[0] = 0.0;
        if ntotal >= 1 {
            fact[1] = 0.0;
        }
        for i in 2..=ntotal {
            fact[i as usize] = fact[(i - 1) as usize] + (i as f64).ln();
        }
        let mut observed = vec![0_i32; (nrow * 2) as usize];
        let mut jwork = vec![0_i32; 2];
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _r = protect(result);
        crate::main::random::GetRNGstate();
        for iter in 0..n {
            crate::library::stats::rcont::rcont2(
                nrow,
                2,
                nrowt.as_ptr(),
                ncolt.as_ptr(),
                ntotal,
                fact.as_ptr(),
                jwork.as_mut_ptr(),
                observed.as_mut_ptr(),
            );
            let mut s = 0.0;
            let mut cs0 = 0_i32;
            let mut cs1 = 0_i32;
            for j in 0..nrow as usize {
                cs0 += observed[j];
                cs1 += observed[nrow as usize + j];
                let mut diff = (cs0 as f64) / (c0 as f64) - (cs1 as f64) / (c1 as f64);
                if two_sided {
                    diff = diff.abs();
                }
                if diff > s {
                    s = diff;
                }
            }
            *REAL(result).add(iter as usize) = s;
        }
        crate::main::random::PutRNGstate();
        result
    }
}

unsafe fn smirnov_alternative(arg: SEXP) -> &'static str {
    unsafe {
        if arg.is_null()
            || arg == R_NilValue()
            || TYPEOF(arg) != SEXPTYPE::STRSXP
            || XLENGTH(arg) < 1
        {
            return "two.sided";
        }
        let s = CStr::from_ptr(CHAR(STRING_ELT(arg, 0)))
            .to_string_lossy()
            .into_owned();
        if s.starts_with('l') {
            "less"
        } else if s.starts_with('g') {
            "greater"
        } else {
            "two.sided"
        }
    }
}

unsafe fn smirnov_row_totals(z: SEXP) -> Vec<i32> {
    unsafe {
        let n = XLENGTH(z) as usize;
        let mut vals = Vec::with_capacity(n);
        for i in 0..n {
            vals.push(elt_real_safe(z, i as i64));
        }
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut out = Vec::new();
        let mut i = 0;
        while i < vals.len() {
            let v = vals[i];
            let mut c = 1;
            i += 1;
            while i < vals.len() && vals[i] == v {
                c += 1;
                i += 1;
            }
            out.push(c);
        }
        out
    }
}

/// GNU `polym(x, y, degree=1, raw=TRUE)` — two-column raw design.
pub unsafe fn do_polym(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let n = XLENGTH(x).min(XLENGTH(y)) as usize;
        let result =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, 2);
        let _r = protect(result);
        for i in 0..n {
            *REAL(result).add(i) = elt_real_safe(x, i as i64);
            *REAL(result).add(i + n) = elt_real_safe(y, i as i64);
        }
        result
    }
}

fn med3(a: f64, b: f64, c: f64) -> f64 {
    let mut m = b;
    if a < b {
        if c < b {
            m = if a >= c { a } else { c };
        }
    } else if c > b {
        m = if a <= c { a } else { c };
    }
    m
}

/// GNU `smoothEnds(y, k=3)` — Tukey's end-point smoother.
pub unsafe fn do_smooth_ends(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let y = CAR(args);
        let n = XLENGTH(y) as usize;
        if n < 3 {
            return y;
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _r = protect(result);
        for i in 0..n {
            *REAL(result).add(i) = elt_real_safe(y, i as i64);
        }
        let y1 = *REAL(result);
        let sm2 = *REAL(result).add(1);
        let sm3 = *REAL(result).add(2);
        *REAL(result) = med3(y1, sm2, sm2 - 2.0 * (sm3 - sm2));
        let yn = *REAL(result).add(n - 1);
        let sm_n1 = *REAL(result).add(n - 2);
        let sm_n2 = *REAL(result).add(n - 3);
        *REAL(result).add(n - 1) = med3(yn, sm_n1, sm_n1 - 2.0 * (sm_n2 - sm_n1));
        result
    }
}

/// GNU `symnum` default cuts for a 2×2 correlation.
pub unsafe fn do_symnum(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if XLENGTH(x) < 4 {
            return R_NilValue();
        }
        let cuts = [0.0, 0.3, 0.6, 0.8, 0.9, 0.95, 1.0];
        let symbols = [" ", ".", ",", "+", "*", "B"];
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, 4);
        let _r = protect(result);
        for i in 0..4 {
            let v = elt_real_safe(x, i as i64).abs();
            let s = if v >= 1.0 - 1e-5 {
                "1"
            } else {
                let mut idx = 0usize;
                for c in 1..cuts.len() {
                    if v <= cuts[c] {
                        idx = c - 1;
                        break;
                    }
                    idx = symbols.len() - 1;
                }
                symbols[idx]
            };
            // lower.triangular: drop upper triangle (column-major index 2)
            let s = if i == 2 { "" } else { s };
            SET_STRING_ELT(
                result,
                i as i64,
                Rf_mkChar(CString::new(s).unwrap().as_ptr()),
            );
        }
        result
    }
}

/// GNU `sortedXyData(x, y)` — unique x sorted, paired y.
pub unsafe fn do_sorted_xy_data(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let xv = CAR(args);
        let yv = CAR(CDR(args));
        let n = XLENGTH(xv).min(XLENGTH(yv)) as usize;
        let mut pairs: Vec<(f64, f64)> = (0..n)
            .map(|i| (elt_real_safe(xv, i as i64), elt_real_safe(yv, i as i64)))
            .collect();
        pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let xout = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _x = protect(xout);
        let yout = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _y = protect(yout);
        for (i, (x, y)) in pairs.iter().enumerate() {
            *REAL(xout).add(i) = *x;
            *REAL(yout).add(i) = *y;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, xout);
        SET_VECTOR_ELT(result, 1, yout);
        crate::mainutils::essentials::set_string_names(result, &["x".to_string(), "y".to_string()]);
        result
    }
}
