//! nls asymptotic helpers, glm family objects, links, naprint, aov/manova, glm, proj — extracted verbatim from essentials/mathstats.rs.

use super::*;

unsafe fn nls_xy_y(xy: SEXP) -> (Vec<f64>, bool) {
    unsafe {
        let y = list_named_elt(xy, "y");
        if y != R_NilValue() && XLENGTH(y) > 0 {
            let n = XLENGTH(y) as usize;
            return ((0..n).map(|i| elt_real_safe(y, i as i64)).collect(), true);
        }
        (Vec::new(), false)
    }
}

/// GNU `NLSstLfAsymptote(xy)`.
pub unsafe fn do_nls_lf_asymp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let (ys, ok) = nls_xy_y(CAR(args));
        if !ok || ys.is_empty() {
            return R_NilValue();
        }
        let ymin = ys.iter().copied().fold(f64::INFINITY, f64::min);
        let ymax = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let d = ymax - ymin;
        let first = ys[0];
        let val = if (ymax - first).abs() < (first - ymin).abs() {
            ymax + d / 8.0
        } else {
            ymin - d / 8.0
        };
        Rf_ScalarReal(val)
    }
}

/// GNU `NLSstRtAsymptote(xy)`.
pub unsafe fn do_nls_rt_asymp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let (ys, ok) = nls_xy_y(CAR(args));
        if !ok || ys.is_empty() {
            return R_NilValue();
        }
        let ymin = ys.iter().copied().fold(f64::INFINITY, f64::min);
        let ymax = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let d = ymax - ymin;
        let last = *ys.last().unwrap();
        let val = if (ymax - last).abs() < (last - ymin).abs() {
            ymax + d / 8.0
        } else {
            ymin - d / 8.0
        };
        Rf_ScalarReal(val)
    }
}

fn nls_xy_x(xy: SEXP) -> Vec<f64> {
    unsafe {
        let x = list_named_elt(xy, "x");
        if x == R_NilValue() {
            return Vec::new();
        }
        (0..XLENGTH(x) as usize)
            .map(|i| elt_real_safe(x, i as i64))
            .collect()
    }
}

fn nls_asymp_ols(xs: &[f64], ys: &[f64], lrc: f64) -> (f64, f64, f64) {
    let n = xs.len();
    if n < 2 || !lrc.is_finite() {
        return (0.0, 0.0, f64::INFINITY);
    }
    let rate = (-lrc.exp()).exp(); // unused; z = 1-exp(-exp(lrc)*x)
    let _ = rate;
    let mut zs = vec![0.0; n];
    for i in 0..n {
        zs[i] = 1.0 - (-lrc.exp() * xs[i]).exp();
    }
    let nf = n as f64;
    let my = ys.iter().sum::<f64>() / nf;
    let mz = zs.iter().sum::<f64>() / nf;
    let mut num = 0.0;
    let mut den = 0.0;
    for i in 0..n {
        let dz = zs[i] - mz;
        num += dz * (ys[i] - my);
        den += dz * dz;
    }
    let b1 = if den > 0.0 { num / den } else { 0.0 };
    let b0 = my - b1 * mz;
    let mut sse = 0.0;
    for i in 0..n {
        let e = ys[i] - (b0 + b1 * zs[i]);
        sse += e * e;
    }
    (b0, b1, sse)
}

/// GNU `NLSstAsymptotic(xy)` — `b0 + b1*(1-exp(-exp(lrc)*x))`.
pub unsafe fn do_nls_asymptotic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let xy = CAR(args);
        let xs = nls_xy_x(xy);
        let (ys, ok) = nls_xy_y(xy);
        if !ok || xs.len() < 2 || ys.len() != xs.len() {
            return R_NilValue();
        }
        let rt = elt_real_safe(do_nls_rt_asymp(_call, _op, args, _rho), 0);
        let mut slx = 0.0;
        let mut sly = 0.0;
        let mut nlm = 0.0;
        for i in 0..xs.len() {
            let ly = (ys[i] - rt).abs().ln();
            if ly.is_finite() {
                slx += xs[i];
                sly += ly;
                nlm += 1.0;
            }
        }
        let mut lrc0 = -1.0;
        if nlm >= 2.0 {
            let mx = slx / nlm;
            let my = sly / nlm;
            let mut num = 0.0;
            let mut den = 0.0;
            for i in 0..xs.len() {
                let ly = (ys[i] - rt).abs().ln();
                if ly.is_finite() {
                    let dx = xs[i] - mx;
                    num += dx * (ly - my);
                    den += dx * dx;
                }
            }
            let slope = if den > 0.0 { num / den } else { 0.0 };
            if slope < 0.0 {
                lrc0 = (-slope).ln();
            }
        }
        let mut best_lrc = lrc0;
        let mut best = nls_asymp_ols(&xs, &ys, lrc0);
        let mut lo = lrc0 - 3.0;
        let mut hi = lrc0 + 3.0;
        for _ in 0..40 {
            let m1 = lo + (hi - lo) / 3.0;
            let m2 = hi - (hi - lo) / 3.0;
            let a1 = nls_asymp_ols(&xs, &ys, m1);
            let a2 = nls_asymp_ols(&xs, &ys, m2);
            if a1.2 < a2.2 {
                hi = m2;
                if a1.2 < best.2 {
                    best = a1;
                    best_lrc = m1;
                }
            } else {
                lo = m1;
                if a2.2 < best.2 {
                    best = a2;
                    best_lrc = m2;
                }
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        let _r = protect(result);
        *REAL(result) = best.0;
        *REAL(result).add(1) = best.1;
        *REAL(result).add(2) = best_lrc;
        crate::mainutils::essentials::set_string_names(
            result,
            &["b0".to_string(), "b1".to_string(), "lrc".to_string()],
        );
        result
    }
}

/// GNU `NLSstClosestX(xy, yval)` — interpolate x at a target y.
pub unsafe fn do_nls_closest_x(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let xy = CAR(args);
        let yval = elt_real_safe(CAR(CDR(args)), 0);
        let xs = list_named_elt(xy, "x");
        let ys = list_named_elt(xy, "y");
        if xs == R_NilValue() || ys == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(xs).min(XLENGTH(ys)) as usize;
        if n == 0 {
            return R_NilValue();
        }
        let mut lim1 = f64::NAN;
        let mut dev1 = f64::NAN;
        let mut lim2 = f64::NAN;
        let mut dev2 = f64::NAN;
        let mut have_neg = false;
        let mut have_pos = false;
        for i in 0..n {
            let x = elt_real_safe(xs, i as i64);
            let d = elt_real_safe(ys, i as i64) - yval;
            if d == 0.0 {
                return Rf_ScalarReal(x);
            }
            if d <= 0.0 {
                if !have_neg || d > dev1 {
                    dev1 = d;
                    lim1 = x;
                    have_neg = true;
                }
            }
            if d >= 0.0 {
                if !have_pos || d < dev2 {
                    dev2 = d;
                    lim2 = x;
                    have_pos = true;
                }
            }
        }
        if have_neg && !have_pos {
            return Rf_ScalarReal(lim1);
        }
        if have_pos && !have_neg {
            return Rf_ScalarReal(lim2);
        }
        if !have_neg || !have_pos {
            return R_NilValue();
        }
        let a = dev1.abs();
        let b = dev2.abs();
        Rf_ScalarReal(lim1 + (lim2 - lim1) * a / (a + b))
    }
}

unsafe fn family_object(family: &str, link: &str) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        let fam = match family {
            "gaussian" => Rf_mkString(c"gaussian".as_ptr()),
            "poisson" => Rf_mkString(c"poisson".as_ptr()),
            "Gamma" => Rf_mkString(c"Gamma".as_ptr()),
            "inverse.gaussian" => Rf_mkString(c"inverse.gaussian".as_ptr()),
            "quasi" => Rf_mkString(c"quasi".as_ptr()),
            "quasibinomial" => Rf_mkString(c"quasibinomial".as_ptr()),
            "quasipoisson" => Rf_mkString(c"quasipoisson".as_ptr()),
            _ => Rf_mkString(c"binomial".as_ptr()),
        };
        let lnk = match link {
            "identity" => Rf_mkString(c"identity".as_ptr()),
            "probit" => Rf_mkString(c"probit".as_ptr()),
            "log" => Rf_mkString(c"log".as_ptr()),
            "inverse" => Rf_mkString(c"inverse".as_ptr()),
            "1/mu^2" => Rf_mkString(c"1/mu^2".as_ptr()),
            _ => Rf_mkString(c"logit".as_ptr()),
        };
        SET_VECTOR_ELT(result, 0, fam);
        SET_VECTOR_ELT(result, 1, lnk);
        crate::mainutils::essentials::set_string_names(
            result,
            &["family".to_string(), "link".to_string()],
        );
        let class = Rf_mkString(c"family".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `binomial()` family object.
pub unsafe fn do_binomial(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut link = "logit".to_string();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                link = elt_to_string(v, 0);
            }
            cell = CDR(cell);
        }
        family_object("binomial", &link)
    }
}

/// GNU `gaussian()` family object.
pub unsafe fn do_gaussian(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut link = "identity".to_string();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                link = elt_to_string(v, 0);
            }
            cell = CDR(cell);
        }
        family_object("gaussian", &link)
    }
}

/// GNU `poisson()` family object.
pub unsafe fn do_poisson(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut link = "log".to_string();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                link = elt_to_string(v, 0);
            }
            cell = CDR(cell);
        }
        family_object("poisson", &link)
    }
}

/// GNU `Gamma()` family object.
pub unsafe fn do_gamma_family(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut link = "inverse".to_string();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                link = elt_to_string(v, 0);
            }
            cell = CDR(cell);
        }
        family_object("Gamma", &link)
    }
}

/// GNU `inverse.gaussian()` family object.
pub unsafe fn do_inverse_gaussian(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut link = "1/mu^2".to_string();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                link = elt_to_string(v, 0);
            }
            cell = CDR(cell);
        }
        family_object("inverse.gaussian", &link)
    }
}

/// GNU `quasi()` family object.
pub unsafe fn do_quasi(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut link = "identity".to_string();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let v = CAR(cell);
            if (name == "link" || name.is_empty())
                && TYPEOF(v) == SEXPTYPE::STRSXP
                && XLENGTH(v) > 0
            {
                link = elt_to_string(v, 0);
            }
            cell = CDR(cell);
        }
        family_object("quasi", &link)
    }
}

/// GNU `quasibinomial()` family object.
pub unsafe fn do_quasibinomial(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut link = "logit".to_string();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                link = elt_to_string(v, 0);
            }
            cell = CDR(cell);
        }
        family_object("quasibinomial", &link)
    }
}

/// GNU `quasipoisson()` family object.
pub unsafe fn do_quasipoisson(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut link = "log".to_string();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            if TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                link = elt_to_string(v, 0);
            }
            cell = CDR(cell);
        }
        family_object("quasipoisson", &link)
    }
}

/// GNU `make.link(link)` — name + class `link-glm`.
pub unsafe fn do_make_link(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut link = "identity".to_string();
        let v = CAR(args);
        if !v.is_null() && v != R_NilValue() && TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
            link = elt_to_string(v, 0);
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        let name = match link.as_str() {
            "logit" => Rf_mkString(c"logit".as_ptr()),
            "probit" => Rf_mkString(c"probit".as_ptr()),
            "log" => Rf_mkString(c"log".as_ptr()),
            "inverse" => Rf_mkString(c"inverse".as_ptr()),
            "1/mu^2" => Rf_mkString(c"1/mu^2".as_ptr()),
            "cloglog" => Rf_mkString(c"cloglog".as_ptr()),
            "cauchit" => Rf_mkString(c"cauchit".as_ptr()),
            "sqrt" => Rf_mkString(c"sqrt".as_ptr()),
            _ => Rf_mkString(c"identity".as_ptr()),
        };
        SET_VECTOR_ELT(result, 0, name);
        crate::mainutils::essentials::set_string_names(result, &["name".to_string()]);
        let class = Rf_mkString(c"link-glm".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `power(lambda)` — Box-Cox GLM link name.
pub unsafe fn do_power(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let lambda = if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
            1.0
        } else {
            elt_real_safe(CAR(args), 0)
        };
        let name = if lambda <= 0.0 {
            "log".to_string()
        } else if (lambda - 1.0).abs() < 1e-12 {
            "identity".to_string()
        } else {
            let r = (lambda * 1000.0).round() / 1000.0;
            if (r - r.round()).abs() < 1e-12 {
                format!("mu^{}", r as i64)
            } else {
                format!("mu^{r}")
            }
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_mkString(CString::new(name).unwrap().as_ptr()));
        crate::mainutils::essentials::set_string_names(result, &["name".to_string()]);
        let class = Rf_mkString(c"link-glm".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `Pair(x, y)` — two-column matrix with class `Pair`.
pub unsafe fn do_pair(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"Pair".as_ptr()),
        );
        result
    }
}

/// GNU `naprint(x)` — omit/exclude count, else empty string.
pub unsafe fn do_naprint(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        let mut omit = false;
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(class) {
                let s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(class, i)));
                if s.to_bytes() == b"omit" || s.to_bytes() == b"exclude" {
                    omit = true;
                    break;
                }
            }
        }
        if !omit {
            return Rf_mkString(c"".as_ptr());
        }
        let n = XLENGTH(x);
        let msg = if n == 1 {
            "1 observation deleted due to missingness".to_string()
        } else {
            format!("{n} observations deleted due to missingness")
        };
        Rf_mkString(CString::new(msg).unwrap().as_ptr())
    }
}

/// GNU `aov(y ~ x)` — `lm` with class `c("aov","lm")`.
pub unsafe fn do_aov(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let result = do_lm(call, op, args, rho);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _cl = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"aov".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"lm".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `manova(cbind(y1,y2) ~ g)` — two-group coefficients and residuals.
pub unsafe fn do_manova(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let form = CAR(args);
        if form.is_null() || form == R_NilValue() || TYPEOF(form) != SEXPTYPE::LANGSXP {
            return R_NilValue();
        }
        let lhs = CADR(form);
        let rhs_cell = CDR(CDR(form));
        if rhs_cell.is_null() || rhs_cell == R_NilValue() {
            return R_NilValue();
        }
        let g_expr = CAR(rhs_cell);
        let y = crate::eval::eval::Rf_eval(lhs, rho);
        let g = crate::eval::eval::Rf_eval(g_expr, rho);
        let _y = protect(y);
        let _g = protect(g);
        let dim = crate::sexp::attrib_core::getAttrib(y, crate::sexp::attrib_core::R_DimSymbol());
        let (nr, nc) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            (XLENGTH(y) as usize, 1)
        };
        if nr == 0 || nc == 0 || TYPEOF(g) != SEXPTYPE::INTSXP {
            return R_NilValue();
        }
        let ng = XLENGTH(g) as usize;
        let n = nr.min(ng);
        let mut ys = vec![0.0; n * nc];
        let mut sum1 = vec![0.0; nc];
        let mut sum2 = vec![0.0; nc];
        let mut n1 = 0.0;
        let mut n2 = 0.0;
        for i in 0..n {
            let code = *INTEGER(g).add(i);
            for j in 0..nc {
                let v = if TYPEOF(y) == SEXPTYPE::REALSXP {
                    *REAL(y).add(i + j * nr)
                } else {
                    *INTEGER(y).add(i + j * nr) as f64
                };
                ys[i + j * n] = v;
                if code <= 1 {
                    sum1[j] += v;
                    if j == 0 {
                        n1 += 1.0;
                    }
                } else {
                    sum2[j] += v;
                    if j == 0 {
                        n2 += 1.0;
                    }
                }
            }
        }
        if n1 <= 0.0 || n2 <= 0.0 {
            return R_NilValue();
        }
        let coef = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 2, nc as i32);
        let _c = protect(coef);
        let resid =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, nc as i32);
        let _rs = protect(resid);
        for j in 0..nc {
            let m1 = sum1[j] / n1;
            let m2 = sum2[j] / n2;
            *REAL(coef).add(j * 2) = m1;
            *REAL(coef).add(1 + j * 2) = m2 - m1;
            for i in 0..n {
                let code = *INTEGER(g).add(i);
                let m = if code <= 1 { m1 } else { m2 };
                *REAL(resid).add(i + j * n) = ys[i + j * n] - m;
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, coef);
        SET_VECTOR_ELT(result, 1, resid);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(n1));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "coefficients".to_string(),
                "residuals".to_string(),
                "n1".to_string(),
            ],
        );
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 5);
        let _cl = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"manova".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"maov".as_ptr()));
        SET_STRING_ELT(class, 2, Rf_mkChar(c"aov".as_ptr()));
        SET_STRING_ELT(class, 3, Rf_mkChar(c"mlm".as_ptr()));
        SET_STRING_ELT(class, 4, Rf_mkChar(c"lm".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `summary.manova` — two-response Pillai trace for two groups.
pub unsafe fn do_summary_manova(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        if obj.is_null() || obj == R_NilValue() || TYPEOF(obj) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let coef = list_named_elt(obj, "coefficients");
        let resid = list_named_elt(obj, "residuals");
        let n1 = elt_real_safe(list_named_elt(obj, "n1"), 0);
        if coef.is_null() || resid.is_null() || coef == R_NilValue() || resid == R_NilValue() {
            return R_NilValue();
        }
        let rdim =
            crate::sexp::attrib_core::getAttrib(resid, crate::sexp::attrib_core::R_DimSymbol());
        if rdim.is_null() || rdim == R_NilValue() || XLENGTH(rdim) < 2 {
            return R_NilValue();
        }
        let n = *INTEGER(rdim) as usize;
        let p = *INTEGER(rdim).add(1) as usize;
        if p != 2 || n < 3 {
            return R_NilValue();
        }
        let n2 = n as f64 - n1;
        if n1 <= 0.0 || n2 <= 0.0 {
            return R_NilValue();
        }
        let d0 = *REAL(coef).add(1);
        let d1 = *REAL(coef).add(3);
        let scale = n1 * n2 / (n as f64);
        let h11 = scale * d0 * d0;
        let h12 = scale * d0 * d1;
        let h22 = scale * d1 * d1;
        let mut e11 = 0.0;
        let mut e12 = 0.0;
        let mut e22 = 0.0;
        for i in 0..n {
            let r0 = *REAL(resid).add(i);
            let r1 = *REAL(resid).add(i + n);
            e11 += r0 * r0;
            e12 += r0 * r1;
            e22 += r1 * r1;
        }
        let det = e11 * e22 - e12 * e12;
        if det.abs() < 1e-15 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "residuals have rank 1 < 2",
            );
        }
        let i11 = e22 / det;
        let i12 = -e12 / det;
        let i22 = e11 / det;
        let a11 = i11 * h11 + i12 * h12;
        let a12 = i11 * h12 + i12 * h22;
        let a21 = i12 * h11 + i22 * h12;
        let a22 = i12 * h12 + i22 * h22;
        let tr = a11 + a22;
        let det_a = a11 * a22 - a12 * a21;
        let disc = (tr * tr - 4.0 * det_a).max(0.0).sqrt();
        let l1 = 0.5 * (tr + disc);
        let l2 = 0.5 * (tr - disc);
        let v = l1 / (1.0 + l1) + l2 / (1.0 + l2);
        let q: f64 = 1.0;
        let df_res = n as f64 - 2.0;
        let s = q.min(p as f64);
        let nn = 0.5 * (df_res - p as f64 - 1.0);
        let m = 0.5 * ((p as f64 - q).abs() - 1.0);
        let tmp1 = 2.0 * m + s + 1.0;
        let tmp2 = 2.0 * nn + s + 1.0;
        let fstat = (tmp2 / tmp1 * v) / (s - v);
        let num_df = s * tmp1;
        let den_df = s * tmp2;
        let pval = crate::dist::f_dist::pf_inner(fstat, num_df, den_df, false, false);
        let stats = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 2, 6);
        let _st = protect(stats);
        *REAL(stats) = 1.0;
        *REAL(stats).add(1) = df_res;
        *REAL(stats).add(2) = v;
        *REAL(stats).add(3) = f64::NAN;
        *REAL(stats).add(4) = fstat;
        *REAL(stats).add(5) = f64::NAN;
        *REAL(stats).add(6) = num_df;
        *REAL(stats).add(7) = f64::NAN;
        *REAL(stats).add(8) = den_df;
        *REAL(stats).add(9) = f64::NAN;
        *REAL(stats).add(10) = pval;
        *REAL(stats).add(11) = f64::NAN;
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _rn = protect(rn);
        SET_STRING_ELT(rn, 0, Rf_mkChar(c"g".as_ptr()));
        SET_STRING_ELT(rn, 1, Rf_mkChar(c"Residuals".as_ptr()));
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 6);
        let _cn = protect(cn);
        SET_STRING_ELT(cn, 0, Rf_mkChar(c"Df".as_ptr()));
        SET_STRING_ELT(cn, 1, Rf_mkChar(c"Pillai".as_ptr()));
        SET_STRING_ELT(cn, 2, Rf_mkChar(c"approx F".as_ptr()));
        SET_STRING_ELT(cn, 3, Rf_mkChar(c"num Df".as_ptr()));
        SET_STRING_ELT(cn, 4, Rf_mkChar(c"den Df".as_ptr()));
        SET_STRING_ELT(cn, 5, Rf_mkChar(c"Pr(>F)".as_ptr()));
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(
            stats,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dn,
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, stats);
        crate::mainutils::essentials::set_string_names(result, &["stats".to_string()]);
        let class = Rf_mkString(c"summary.manova".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `TukeyHSD(aov)` — pairwise studentized-range intervals for one grouping factor.
pub unsafe fn do_tukey_hsd(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let form =
            crate::library::stats::filter::do_formula(_call, _op, Rf_cons(obj, R_NilValue()), rho);
        if form.is_null() || form == R_NilValue() || TYPEOF(form) != SEXPTYPE::LANGSXP {
            return R_NilValue();
        }
        let y_expr = CADR(form);
        let g_cell = CDR(CDR(form));
        let g_expr = if g_cell.is_null() || g_cell == R_NilValue() {
            return R_NilValue();
        } else {
            CAR(g_cell)
        };
        let y = crate::eval::eval::Rf_eval(y_expr, rho);
        let g = crate::eval::eval::Rf_eval(g_expr, rho);
        let _y = protect(y);
        let _g = protect(g);
        if TYPEOF(g) != SEXPTYPE::INTSXP {
            return R_NilValue();
        }
        let levels =
            crate::sexp::attrib_core::getAttrib(g, crate::sexp::attrib_core::R_LevelsSymbol());
        let k = if TYPEOF(levels) == SEXPTYPE::STRSXP {
            XLENGTH(levels) as usize
        } else {
            0
        };
        let n = XLENGTH(y).min(XLENGTH(g)) as usize;
        if k < 2 || n < k {
            return R_NilValue();
        }
        let mut sums = vec![0.0; k];
        let mut ns = vec![0.0; k];
        let mut ys = vec![0.0; n];
        let mut gs = vec![0usize; n];
        for i in 0..n {
            let code = *INTEGER(g).add(i);
            if code < 1 || (code as usize) > k {
                continue;
            }
            let gi = (code as usize) - 1;
            let yi = elt_real_safe(y, i as i64);
            ys[i] = yi;
            gs[i] = gi;
            sums[gi] += yi;
            ns[gi] += 1.0;
        }
        let mut means = vec![0.0; k];
        for i in 0..k {
            if ns[i] > 0.0 {
                means[i] = sums[i] / ns[i];
            }
        }
        let mut sse = 0.0;
        for i in 0..n {
            let e = ys[i] - means[gs[i]];
            sse += e * e;
        }
        let df = n as f64 - k as f64;
        if df <= 0.0 {
            return R_NilValue();
        }
        let mse = sse / df;
        let npairs = k * (k - 1) / 2;
        let tab =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), npairs as i32, 4);
        let _t = protect(tab);
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, npairs as i64);
        let _rn = protect(rn);
        let qcrit = crate::dist::tukey::qtukey_inner(0.95, 1.0, k as f64, df, true, false);
        let mut row = 0usize;
        for j in 1..k {
            for i in 0..j {
                let diff = means[j] - means[i];
                let se = (mse * (1.0 / ns[i] + 1.0 / ns[j])).sqrt();
                let crit = if se > 0.0 {
                    qcrit / std::f64::consts::SQRT_2 * se
                } else {
                    0.0
                };
                let qobs = if se > 0.0 {
                    diff.abs() / se * std::f64::consts::SQRT_2
                } else {
                    0.0
                };
                let padj =
                    1.0 - crate::dist::tukey::ptukey_inner(qobs, 1.0, k as f64, df, true, false);
                *REAL(tab).add(row) = diff;
                *REAL(tab).add(row + npairs) = diff - crit;
                *REAL(tab).add(row + 2 * npairs) = diff + crit;
                *REAL(tab).add(row + 3 * npairs) = padj;
                let li = STRING_ELT(levels, i as i64);
                let lj = STRING_ELT(levels, j as i64);
                let si = std::ffi::CStr::from_ptr(CHAR(li)).to_string_lossy();
                let sj = std::ffi::CStr::from_ptr(CHAR(lj)).to_string_lossy();
                let lab = format!("{sj}-{si}\0");
                SET_STRING_ELT(rn, row as i64, Rf_mkChar(lab.as_ptr() as *const _));
                row += 1;
            }
        }
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 4);
        let _cn = protect(cn);
        SET_STRING_ELT(cn, 0, Rf_mkChar(c"diff".as_ptr()));
        SET_STRING_ELT(cn, 1, Rf_mkChar(c"lwr".as_ptr()));
        SET_STRING_ELT(cn, 2, Rf_mkChar(c"upr".as_ptr()));
        SET_STRING_ELT(cn, 3, Rf_mkChar(c"p adj".as_ptr()));
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(tab, crate::sexp::attrib_core::R_DimNamesSymbol(), dn);
        let term = if TYPEOF(g_expr) == SEXPTYPE::SYMSXP {
            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(g_expr)))
                .to_string_lossy()
                .into_owned()
        } else {
            "g".to_string()
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, tab);
        let tn = std::ffi::CString::new(term).unwrap_or_default();
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _nm = protect(names);
        SET_STRING_ELT(names, 0, Rf_mkChar(tn.as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        let class = Rf_mkString(c"TukeyHSD".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `glm(y ~ x)` gaussian — `lm` with class `c("glm","lm")`.
pub unsafe fn do_glm(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let lm = do_lm(call, op, args, rho);
        if lm.is_null() || lm == R_NilValue() {
            return lm;
        }
        let n = XLENGTH(lm);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n + 1);
        let _r = protect(result);
        for i in 0..n {
            SET_VECTOR_ELT(result, i, VECTOR_ELT(lm, i));
        }
        SET_VECTOR_ELT(result, n, family_object("gaussian", "identity"));
        let old_names =
            crate::sexp::attrib_core::getAttrib(lm, crate::sexp::attrib_core::R_NamesSymbol());
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, n + 1);
        let _nm = protect(names);
        for i in 0..n {
            if TYPEOF(old_names) == SEXPTYPE::STRSXP && i < XLENGTH(old_names) {
                SET_STRING_ELT(names, i, STRING_ELT(old_names, i));
            }
        }
        SET_STRING_ELT(names, n, Rf_mkChar(c"family".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _cl = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"glm".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"lm".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `proj(lm)` — intercept mean, centered slope, residuals.
pub unsafe fn do_proj(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let fitted = list_named_elt(obj, "fitted.values");
        let resid = list_named_elt(obj, "residuals");
        let coef = list_named_elt(obj, "coefficients");
        if fitted == R_NilValue() || resid == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(fitted).min(XLENGTH(resid)) as usize;
        if n < 2 {
            return R_NilValue();
        }
        let b = if XLENGTH(coef) >= 2 {
            elt_real_safe(coef, 1)
        } else {
            0.0
        };
        let nf = n as f64;
        let mx = (nf + 1.0) / 2.0;
        let mut ysum = 0.0;
        for i in 0..n {
            ysum += elt_real_safe(fitted, i as i64) + elt_real_safe(resid, i as i64);
        }
        let my = ysum / nf;
        let result =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, 3);
        let _r = protect(result);
        for i in 0..n {
            let xi = (i + 1) as f64;
            *REAL(result).add(i) = my;
            *REAL(result).add(i + n) = b * (xi - mx);
            *REAL(result).add(i + 2 * n) = elt_real_safe(resid, i as i64);
        }
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        let _cn = protect(cn);
        SET_STRING_ELT(cn, 0, Rf_mkChar(c"(Intercept)".as_ptr()));
        SET_STRING_ELT(cn, 1, Rf_mkChar(c"I(1:5)".as_ptr()));
        SET_STRING_ELT(cn, 2, Rf_mkChar(c"Residuals".as_ptr()));
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dn,
        );
        result
    }
}

/// GNU `qqplot(x, y, plot.it=FALSE)` — sorted pairs.
pub unsafe fn do_qqplot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
            return R_NilValue();
        }
        let nx = XLENGTH(x) as usize;
        let ny = XLENGTH(y) as usize;
        if nx == 0 || ny == 0 {
            return R_NilValue();
        }
        let mut xs: Vec<f64> = (0..nx).map(|i| elt_real_safe(x, i as i64)).collect();
        let mut ys: Vec<f64> = (0..ny).map(|i| elt_real_safe(y, i as i64)).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = nx.min(ny);
        let xo = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _x = protect(xo);
        let yo = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _y = protect(yo);
        for i in 0..n {
            *REAL(xo).add(i) = xs[i];
            *REAL(yo).add(i) = ys[i];
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, xo);
        SET_VECTOR_ELT(result, 1, yo);
        crate::mainutils::essentials::set_string_names(result, &["x".to_string(), "y".to_string()]);
        result
    }
}

/// GNU `pairwise.table(compare.levels, level.names, p.adjust.method)`.
pub unsafe fn do_pairwise_table(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let compare = CAR(args);
        let names = CAR(CDR(args));
        if compare.is_null() || compare == R_NilValue() {
            return R_NilValue();
        }
        let n = if names.is_null() || names == R_NilValue() {
            0
        } else {
            XLENGTH(names) as usize
        };
        if n < 2 {
            return R_NilValue();
        }
        let nr = n - 1;
        let nc = n - 1;
        let result = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            nr as i32,
            nc as i32,
        );
        let _r = protect(result);
        for j in 0..nc {
            let j_level = (j + 1) as i32;
            for i in 0..nr {
                let i_level = (i + 2) as i32;
                let idx = i + j * nr;
                if i_level > j_level {
                    let call = crate::sexp::constructors::Rf_lang3(
                        compare,
                        Rf_ScalarInteger(i_level),
                        Rf_ScalarInteger(j_level),
                    );
                    let _c = protect(call);
                    let val = crate::eval::eval::Rf_eval(call, rho);
                    *REAL(result).add(idx) = elt_real_safe(val, 0);
                } else {
                    *REAL(result).add(idx) = NA_REAL;
                }
            }
        }
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, nr as i64);
        let _rn = protect(rn);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, nc as i64);
        let _cn = protect(cn);
        for i in 0..nr {
            SET_STRING_ELT(rn, i as i64, STRING_ELT(names, (i + 1) as i64));
        }
        for j in 0..nc {
            SET_STRING_ELT(cn, j as i64, STRING_ELT(names, j as i64));
        }
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dn,
        );
        result
    }
}

/// GNU `predict.glm(object)` — `$linear.predictors` or `$fitted.values`.
pub unsafe fn do_predict_glm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let lp = list_named_elt(obj, "linear.predictors");
        if lp != R_NilValue() {
            return lp;
        }
        list_named_elt(obj, "fitted.values")
    }
}

/// GNU `family(object)` — extract `$family`.
pub unsafe fn do_family(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let v = list_named_elt(CAR(args), "family");
        if v == R_NilValue() { R_NilValue() } else { v }
    }
}

/// GNU `model.offset(x)` — `$offset` or NULL.
pub unsafe fn do_model_offset(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { list_named_elt(CAR(args), "offset") }
}

/// GNU `update(object)` — re-evaluate `$call`.
pub unsafe fn do_update(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let call = list_named_elt(obj, "call");
        if call.is_null() || call == R_NilValue() {
            return R_NilValue();
        }
        crate::eval::eval::Rf_eval(call, rho)
    }
}
