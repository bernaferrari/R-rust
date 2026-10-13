//! lm.fit / glm.fit / nls and self-start models — extracted verbatim from essentials/mathstats.rs.

use super::*;

/// GNU `lm(y ~ x)` intercept + slope, and owning intercept-only fitting.
pub unsafe fn do_lm(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let _pin = owner
            .pin()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let _arguments = owner
            .sexp(args)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        if let Some(model) = try_lm_intercept(owner, call, args, rho) {
            return model;
        }
        let mut row_data = None;
        let mut row_labels = Vec::new();
        let mut response_roots = Vec::new();
        let first = CAR(args);
        let eval_rho = {
            let mut data = R_NilValue();
            let mut p = CDR(args);
            while !p.is_null() && p != R_NilValue() {
                let tag = TAG(p);
                let name =
                    if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                        std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                            .to_string_lossy()
                            .into_owned()
                    } else {
                        String::new()
                    };
                if name == "data" || (name.is_empty() && data == R_NilValue()) {
                    data = CAR(p);
                    if name == "data" {
                        break;
                    }
                }
                p = CDR(p);
            }
            if !data.is_null() && data != R_NilValue() {
                row_data = Some(
                    owner
                        .sexp(data)
                        .and_then(crate::sexp::object::Sexp::into_owned)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string())),
                );
                crate::mainutils::essentials::data_environment(data, rho)
            } else {
                rho
            }
        };
        let _eval_rho = protect(eval_rho);
        let mut y = first;
        let mut preds: Vec<(String, SEXP)> = Vec::new();
        if !first.is_null() && first != R_NilValue() && TYPEOF(first) == SEXPTYPE::LANGSXP {
            let lhs = CADR(first);
            let rhs = CAR(CDR(CDR(first)));
            if !lhs.is_null() && lhs != R_NilValue() {
                y = crate::eval::eval::Rf_eval(lhs, eval_rho);
            }
            let response = owner
                .sexp(y)
                .and_then(crate::sexp::object::Sexp::into_owned)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            response_roots.push(response.clone());
            let weak = owner
                .weak_owner()
                .unwrap_or_else(|| crate::sexp::context::r_error("lm requires a managed runtime"));
            row_labels = crate::sexp::owner::with_runtime(&weak, |access| {
                super::covratio::row_labels(access, row_data, response.clone())
            })
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
            .unwrap_or_else(|e| crate::sexp::context::r_error(e));

            fn collect_plus(expr: SEXP, out: &mut Vec<SEXP>) {
                unsafe {
                    if expr.is_null() || expr == R_NilValue() {
                        return;
                    }
                    if TYPEOF(expr) == SEXPTYPE::LANGSXP {
                        let op = CAR(expr);
                        let name = if !op.is_null() && TYPEOF(op) == SEXPTYPE::SYMSXP {
                            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(op)))
                                .to_string_lossy()
                                .into_owned()
                        } else {
                            String::new()
                        };
                        if name == "+" {
                            collect_plus(CADR(expr), out);
                            collect_plus(CAR(CDR(CDR(expr))), out);
                            return;
                        }
                    }
                    out.push(expr);
                }
            }
            let mut terms = Vec::new();
            collect_plus(rhs, &mut terms);
            let mut offsets: Vec<SEXP> = Vec::new();
            for term in terms {
                let op_name = if TYPEOF(term) == SEXPTYPE::LANGSXP {
                    let op = CAR(term);
                    if !op.is_null() && TYPEOF(op) == SEXPTYPE::SYMSXP {
                        std::ffi::CStr::from_ptr(CHAR(PRINTNAME(op)))
                            .to_string_lossy()
                            .into_owned()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                if op_name == "offset" {
                    offsets.push(CADR(term));
                    continue;
                }
                if op_name == "Error" {
                    continue;
                }
                let name = if TYPEOF(term) == SEXPTYPE::SYMSXP {
                    std::ffi::CStr::from_ptr(CHAR(PRINTNAME(term)))
                        .to_string_lossy()
                        .into_owned()
                } else {
                    "x".to_string()
                };
                let val = crate::eval::eval::Rf_eval(term, eval_rho);
                preds.push((name, val));
            }
            if !offsets.is_empty() {
                let n = XLENGTH(y) as usize;
                let adjusted = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
                let _a = protect(adjusted);
                for i in 0..n {
                    let mut v = elt_real_safe(y, i as i64);
                    for off in &offsets {
                        let ov = crate::eval::eval::Rf_eval(*off, eval_rho);
                        v -= elt_real_safe(ov, i as i64);
                    }
                    *REAL(adjusted).add(i) = v;
                }
                y = adjusted;
                response_roots.push(
                    owner
                        .sexp(adjusted)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string())),
                );
            }
        } else {
            let x = CAR(CDR(args));
            preds.push(("x".to_string(), x));
        }
        if y.is_null() || y == R_NilValue() || preds.is_empty() {
            return R_NilValue();
        }
        if preds.len() >= 2 {
            let n = {
                let mut n = XLENGTH(y);
                for (_, x) in &preds {
                    if x.is_null() || *x == R_NilValue() {
                        return R_NilValue();
                    }
                    n = n.min(XLENGTH(*x));
                }
                n as usize
            };
            let p = preds.len() + 1;
            if n < p + 1 {
                return R_NilValue();
            }
            let xmat = crate::mainutils::array::allocMatrix(
                SEXPTYPE::REALSXP.as_c_int(),
                n as i32,
                p as i32,
            );
            let _x = protect(xmat);
            for i in 0..n {
                *REAL(xmat).add(i) = 1.0;
            }
            for (j, (_, xv)) in preds.iter().enumerate() {
                for i in 0..n {
                    *REAL(xmat).add(i + (j + 1) * n) = elt_real_safe(*xv, i as i64);
                }
            }
            let qr_args = Rf_cons(xmat, R_NilValue());
            let _qa = protect(qr_args);
            let qr = crate::mainutils::qr::do_qr(call, _op, qr_args, rho);
            let _qr = protect(qr);
            if qr.is_null() || qr == R_NilValue() {
                return R_NilValue();
            }
            let coef_args = Rf_cons(qr, Rf_cons(y, R_NilValue()));
            let _ca = protect(coef_args);
            let coef = crate::mainutils::qr_coef::do_qr_coef(call, _op, coef_args, rho);
            let _c = protect(coef);
            if coef.is_null() || coef == R_NilValue() || XLENGTH(coef) < p as i64 {
                return R_NilValue();
            }
            let mut cnames = vec!["(Intercept)".to_string()];
            cnames.extend(preds.iter().map(|(n, _)| n.clone()));
            set_string_names(coef, &cnames);
            let fit_args = Rf_cons(qr, Rf_cons(y, R_NilValue()));
            let _fa = protect(fit_args);
            let fitted = crate::mainutils::qr_fitted::do_qr_fitted(call, _op, fit_args, rho);
            let _f = protect(fitted);
            let res_args = Rf_cons(qr, Rf_cons(y, R_NilValue()));
            let _ra = protect(res_args);
            let resid = crate::mainutils::qr_fitted::do_qr_resid(call, _op, res_args, rho);
            let _e = protect(resid);
            let q_args = Rf_cons(qr, R_NilValue());
            let _qargs = protect(q_args);
            let qmat = crate::mainutils::qr_q::do_qr_Q(call, _op, q_args, rho);
            let _q = protect(qmat);
            let hats = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
            let _h = protect(hats);
            let qdim =
                crate::sexp::attrib_core::getAttrib(qmat, crate::sexp::attrib_core::R_DimSymbol());
            let qncol = if !qdim.is_null() && TYPEOF(qdim) == SEXPTYPE::INTSXP && XLENGTH(qdim) >= 2
            {
                *INTEGER(qdim).add(1) as usize
            } else {
                0
            };
            let mut sse = 0.0;
            for i in 0..n {
                let e = if !resid.is_null() && TYPEOF(resid) == SEXPTYPE::REALSXP {
                    *REAL(resid).add(i)
                } else {
                    0.0
                };
                sse += e * e;
                let mut hii = 0.0;
                if TYPEOF(qmat) == SEXPTYPE::REALSXP && qncol > 0 {
                    for k in 0..qncol {
                        let qik = *REAL(qmat).add(i + k * n);
                        hii += qik * qik;
                    }
                }
                *REAL(hats).add(i) = hii;
            }
            let rank = p as i32;
            let df = (n as i64) - p as i64;
            let sigma = if df > 0 {
                (sse / df as f64).sqrt()
            } else {
                f64::NAN
            };
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 9);
            let _r = protect(result);
            SET_VECTOR_ELT(result, 0, coef);
            SET_VECTOR_ELT(result, 1, resid);
            SET_VECTOR_ELT(result, 2, fitted);
            SET_VECTOR_ELT(result, 3, Rf_ScalarInteger(rank));
            SET_VECTOR_ELT(result, 4, Rf_ScalarInteger(df as i32));
            SET_VECTOR_ELT(result, 5, Rf_ScalarReal(sigma));
            SET_VECTOR_ELT(result, 6, hats);
            SET_VECTOR_ELT(result, 7, lm_named_call(call));
            SET_VECTOR_ELT(result, 8, qr);
            crate::mainutils::essentials::set_string_names(
                result,
                &[
                    "coefficients".to_string(),
                    "residuals".to_string(),
                    "fitted.values".to_string(),
                    "rank".to_string(),
                    "df.residual".to_string(),
                    "sigma".to_string(),
                    "hat".to_string(),
                    "call".to_string(),
                    "qr".to_string(),
                ],
            );
            if row_labels.len() >= n {
                set_string_names(resid, &row_labels[..n]);
                owner
                    .require_active()
                    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                set_string_names(fitted, &row_labels[..n]);
                owner
                    .require_active()
                    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            }
            let class = Rf_mkString(c"lm".as_ptr());
            let _cl = protect(class);
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
            return result;
        }

        let x = preds[0].1;
        let xname = preds[0].0.clone();
        if y.is_null() || y == R_NilValue() || x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(y).min(XLENGTH(x)) as usize;
        if n < 2 {
            return R_NilValue();
        }
        let mut ys = Vec::with_capacity(n);
        let mut xs = Vec::with_capacity(n);
        let mut x_is_factor = false;
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(class) {
                if std::ffi::CStr::from_ptr(CHAR(STRING_ELT(class, i))).to_bytes() == b"factor" {
                    x_is_factor = true;
                    break;
                }
            }
        }
        for i in 0..n {
            ys.push(elt_real_safe(y, i as i64));
            let mut xi = elt_real_safe(x, i as i64);
            if x_is_factor {
                xi -= 1.0;
            }
            xs.push(xi);
        }
        let mut sxx = 0.0;
        let mut sxy = 0.0;
        let mut sx = 0.0;
        let mut sy = 0.0;
        for i in 0..n {
            sx += xs[i];
            sy += ys[i];
            sxx += xs[i] * xs[i];
            sxy += xs[i] * ys[i];
        }
        let nf = n as f64;
        let Some(inv) = invert2(nf, sx, sx, sxx) else {
            return R_NilValue();
        };
        let b0 = inv.0 * sy + inv.2 * sxy;
        let b1 = inv.1 * sy + inv.3 * sxy;
        let coef = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _c = protect(coef);
        *REAL(coef) = b0;
        *REAL(coef).add(1) = b1;
        set_string_names(coef, &["(Intercept)".to_string(), xname.clone()]);
        let fitted = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _f = protect(fitted);
        let resid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _e = protect(resid);
        let mut sse = 0.0;
        for i in 0..n {
            let fit = b0 + b1 * xs[i];
            let e = ys[i] - fit;
            *REAL(fitted).add(i) = fit;
            *REAL(resid).add(i) = e;
            sse += e * e;
        }
        let df = (n as i64) - 2;
        let sigma = if df > 0 {
            (sse / df as f64).sqrt()
        } else {
            f64::NAN
        };
        let hats = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _h = protect(hats);
        let meanx = sx / nf;
        let sxxc = xs.iter().map(|v| (v - meanx) * (v - meanx)).sum::<f64>();
        for i in 0..n {
            let h = if sxxc > 0.0 {
                1.0 / nf + (xs[i] - meanx) * (xs[i] - meanx) / sxxc
            } else {
                1.0 / nf
            };
            *REAL(hats).add(i) = h;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 8);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, coef);
        SET_VECTOR_ELT(result, 1, resid);
        SET_VECTOR_ELT(result, 2, fitted);
        SET_VECTOR_ELT(result, 3, Rf_ScalarInteger(2));
        SET_VECTOR_ELT(result, 4, Rf_ScalarInteger(df as i32));
        SET_VECTOR_ELT(result, 5, Rf_ScalarReal(sigma));
        SET_VECTOR_ELT(result, 6, hats);
        SET_VECTOR_ELT(result, 7, lm_named_call(call));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "coefficients".to_string(),
                "residuals".to_string(),
                "fitted.values".to_string(),
                "rank".to_string(),
                "df.residual".to_string(),
                "sigma".to_string(),
                "hat".to_string(),
                "call".to_string(),
            ],
        );
        if row_labels.len() >= n {
            set_string_names(resid, &row_labels[..n]);
            owner
                .require_active()
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            set_string_names(fitted, &row_labels[..n]);
            owner
                .require_active()
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        }
        let class = Rf_mkString(c"lm".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        if x_is_factor {
            let lev = crate::sexp::attrib_core::getAttrib(
                x,
                crate::sexp::symbol::Rf_install(c"levels".as_ptr()),
            );
            if !lev.is_null() && TYPEOF(lev) == SEXPTYPE::STRSXP {
                let xl = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
                let _xl = protect(xl);
                SET_VECTOR_ELT(xl, 0, lev);
                crate::mainutils::essentials::set_string_names(xl, std::slice::from_ref(&xname));
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::symbol::Rf_install(c"xlevels".as_ptr()),
                    xl,
                );
            }
        }
        result
    }
}

/// Least squares for designs other than the two-column normal-equation case.
///
/// Uses the same LINPACK QR as GNU `C_Cdqrls`, then unpivots coefficients
/// the way `stats::lm.fit` does (`coef[pivot] <- coef` after NA-filling the
/// rank-deficient tail).
unsafe fn lm_fit_via_qr(x: SEXP, y: SEXP, n: usize) -> SEXP {
    unsafe {
        if XLENGTH(y) != n as i64 {
            return R_NilValue();
        }
        let mut y_values = Vec::with_capacity(n);
        for i in 0..n {
            y_values.push(if TYPEOF(y) == SEXPTYPE::REALSXP {
                *REAL(y).add(i)
            } else {
                elt_real_safe(y, i as i64)
            });
        }
        let tol = Rf_ScalarReal(1.0e-7);
        let _tol = protect(tol);
        let chk = Rf_ScalarLogical(0);
        let _chk = protect(chk);
        let z = crate::library::stats::lm::Cdqrls(x, y, tol, chk);
        if z.is_null() || TYPEOF(z) != SEXPTYPE::VECSXP || XLENGTH(z) < 6 {
            return R_NilValue();
        }
        let _z = protect(z);
        let raw_coef = VECTOR_ELT(z, 1);
        let raw_resid = VECTOR_ELT(z, 2);
        let rank_s = VECTOR_ELT(z, 4);
        let pivot = VECTOR_ELT(z, 5);
        if TYPEOF(raw_coef) != SEXPTYPE::REALSXP
            || TYPEOF(raw_resid) != SEXPTYPE::REALSXP
            || TYPEOF(rank_s) != SEXPTYPE::INTSXP
            || TYPEOF(pivot) != SEXPTYPE::INTSXP
        {
            return R_NilValue();
        }
        let p = XLENGTH(raw_coef) as usize;
        if XLENGTH(raw_resid) != n as i64 || XLENGTH(pivot) != p as i64 || p == 0 {
            return R_NilValue();
        }
        let rank = (*INTEGER(rank_s)).max(0) as usize;
        let mut ordered = vec![0.0; p];
        for i in 0..p {
            ordered[i] = if i < rank {
                *REAL(raw_coef).add(i)
            } else {
                crate::sexp::ffi::NA_REAL
            };
        }
        let mut pivoted = false;
        for i in 0..p {
            if *INTEGER(pivot).add(i) != (i as i32) + 1 {
                pivoted = true;
                break;
            }
        }
        if pivoted {
            let old = ordered.clone();
            for i in 0..p {
                let dest = *INTEGER(pivot).add(i) - 1;
                if dest >= 0 && (dest as usize) < p {
                    ordered[dest as usize] = old[i];
                }
            }
        }
        let coef = Rf_allocVector3(SEXPTYPE::REALSXP, p as i64);
        let _c = protect(coef);
        for (i, value) in ordered.iter().enumerate() {
            *REAL(coef).add(i) = *value;
        }
        let resid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _e = protect(resid);
        let fitted = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _f = protect(fitted);
        for i in 0..n {
            let ri = *REAL(raw_resid).add(i);
            *REAL(resid).add(i) = ri;
            *REAL(fitted).add(i) = y_values[i] - ri;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, coef);
        SET_VECTOR_ELT(result, 1, resid);
        SET_VECTOR_ELT(result, 2, fitted);
        SET_VECTOR_ELT(result, 3, Rf_ScalarInteger(rank as i32));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "coefficients".to_string(),
                "residuals".to_string(),
                "fitted.values".to_string(),
                "rank".to_string(),
            ],
        );
        result
    }
}

/// GNU `lm.fit(x, y)` — two-column normal equations, or LINPACK QR otherwise.
pub unsafe fn do_lm_fit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
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
        if n < 2 || p != 2 || y.is_null() || y == R_NilValue() {
            if p > 2 && n >= 1 && !y.is_null() && y != R_NilValue() {
                return lm_fit_via_qr(x, y, n);
            }
            return R_NilValue();
        }
        let ny = XLENGTH(y) as usize;
        let n = n.min(ny);
        let mut sx = 0.0;
        let mut sy = 0.0;
        let mut sxx = 0.0;
        let mut sxy = 0.0;
        let mut xs = vec![0.0; n];
        let mut ys = vec![0.0; n];
        for i in 0..n {
            let x0 = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else {
                *INTEGER(x).add(i) as f64
            };
            let x1 = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i + n)
            } else {
                *INTEGER(x).add(i + n) as f64
            };
            let yi = elt_real_safe(y, i as i64);
            // fold intercept column into the normal equations via x0
            xs[i] = x1;
            ys[i] = yi;
            sx += x0 * x1;
            sy += x0 * yi;
            sxx += x1 * x1;
            sxy += x1 * yi;
        }
        // If first column is 1s: sx=sum(x), sy=sum(y), a00=n
        let mut a00 = 0.0;
        for i in 0..n {
            let x0 = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else {
                *INTEGER(x).add(i) as f64
            };
            a00 += x0 * x0;
        }
        let Some(inv) = invert2(a00, sx, sx, sxx) else {
            return R_NilValue();
        };
        let b0 = inv.0 * sy + inv.2 * sxy;
        let b1 = inv.1 * sy + inv.3 * sxy;
        let coef = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _c = protect(coef);
        *REAL(coef) = b0;
        *REAL(coef).add(1) = b1;
        let resid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _e = protect(resid);
        let fitted = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _f = protect(fitted);
        for i in 0..n {
            let x0 = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else {
                *INTEGER(x).add(i) as f64
            };
            let fit = b0 * x0 + b1 * xs[i];
            *REAL(fitted).add(i) = fit;
            *REAL(resid).add(i) = ys[i] - fit;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, coef);
        SET_VECTOR_ELT(result, 1, resid);
        SET_VECTOR_ELT(result, 2, fitted);
        SET_VECTOR_ELT(result, 3, Rf_ScalarInteger(2));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "coefficients".to_string(),
                "residuals".to_string(),
                "fitted.values".to_string(),
                "rank".to_string(),
            ],
        );
        result
    }
}

/// GNU `lm.wfit(x, y, w)` — weighted OLS on a two-column design.
pub unsafe fn do_lm_wfit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let w = CAR(CDR(CDR(args)));
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
        if n < 2 || p != 2 || y.is_null() || y == R_NilValue() {
            return R_NilValue();
        }
        let n = n.min(XLENGTH(y) as usize).min(XLENGTH(w) as usize);
        let wtol_cell = CDR(CDR(CDR(args)));
        let wtol = if wtol_cell.is_null()
            || wtol_cell == R_NilValue()
            || CAR(wtol_cell).is_null()
            || CAR(wtol_cell) == R_NilValue()
        {
            0.0
        } else {
            elt_real_safe(CAR(wtol_cell), 0)
        };
        if wtol.is_nan() || wtol < 0.0 {
            crate::mainutils::essentials::base_error("value of 'wtol' must be >= 0".to_owned());
        }
        let mut weights = vec![0.0; n];
        let mut sumw = 0.0;
        for i in 0..n {
            weights[i] = elt_real_safe(w, i as i64);
            sumw += weights[i];
        }
        let thresh = wtol * sumw;
        let mut dropped = false;
        for weight in &mut weights {
            if *weight <= thresh {
                if wtol > 0.0 {
                    dropped = true;
                }
                *weight = 0.0;
            }
        }
        if dropped {
            let msg = std::ffi::CString::new(format!(
                "weights smaller than tolerance wtol*sum(w) = {thresh} treated as zero"
            ))
            .unwrap_or_default();
            crate::mainutils::errors::Rf_warning1(msg.as_ptr());
        }
        let mut a00 = 0.0;
        let mut sx = 0.0;
        let mut sxx = 0.0;
        let mut sy = 0.0;
        let mut sxy = 0.0;
        let mut xs = vec![0.0; n];
        let mut ys = vec![0.0; n];
        let mut x0s = vec![0.0; n];
        for i in 0..n {
            let x0 = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else {
                *INTEGER(x).add(i) as f64
            };
            let x1 = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i + n)
            } else {
                *INTEGER(x).add(i + n) as f64
            };
            let yi = elt_real_safe(y, i as i64);
            let wi = weights[i];
            x0s[i] = x0;
            xs[i] = x1;
            ys[i] = yi;
            a00 += wi * x0 * x0;
            sx += wi * x0 * x1;
            sxx += wi * x1 * x1;
            sy += wi * x0 * yi;
            sxy += wi * x1 * yi;
        }
        let Some(inv) = invert2(a00, sx, sx, sxx) else {
            return R_NilValue();
        };
        let b0 = inv.0 * sy + inv.2 * sxy;
        let b1 = inv.1 * sy + inv.3 * sxy;
        let coef = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _c = protect(coef);
        *REAL(coef) = b0;
        *REAL(coef).add(1) = b1;
        let resid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _e = protect(resid);
        for i in 0..n {
            *REAL(resid).add(i) = ys[i] - (b0 * x0s[i] + b1 * xs[i]);
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, coef);
        SET_VECTOR_ELT(result, 1, resid);
        crate::mainutils::essentials::set_string_names(
            result,
            &["coefficients".to_string(), "residuals".to_string()],
        );
        result
    }
}

/// GNU `glm.fit(x, y, family=poisson())` — IRLS log-link Poisson.
pub unsafe fn do_glm_fit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
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
        if n < 2 || p != 2 || y.is_null() || y == R_NilValue() {
            return R_NilValue();
        }
        let n = n.min(XLENGTH(y) as usize);
        let mut x0s = vec![0.0; n];
        let mut x1s = vec![0.0; n];
        let mut ys = vec![0.0; n];
        for i in 0..n {
            x0s[i] = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else {
                *INTEGER(x).add(i) as f64
            };
            x1s[i] = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i + n)
            } else {
                *INTEGER(x).add(i + n) as f64
            };
            ys[i] = elt_real_safe(y, i as i64);
        }
        let mut b0 = 0.0;
        let mut b1 = 0.0;
        let mut converged = false;
        for _ in 0..25 {
            let mut a00 = 0.0;
            let mut sx = 0.0;
            let mut sxx = 0.0;
            let mut sy = 0.0;
            let mut sxy = 0.0;
            for i in 0..n {
                let eta = b0 * x0s[i] + b1 * x1s[i];
                let mu = eta.exp().max(1e-12);
                let z = eta + (ys[i] - mu) / mu;
                let w = mu;
                a00 += w * x0s[i] * x0s[i];
                sx += w * x0s[i] * x1s[i];
                sxx += w * x1s[i] * x1s[i];
                sy += w * x0s[i] * z;
                sxy += w * x1s[i] * z;
            }
            let Some(inv) = invert2(a00, sx, sx, sxx) else {
                break;
            };
            let nb0 = inv.0 * sy + inv.2 * sxy;
            let nb1 = inv.1 * sy + inv.3 * sxy;
            if (nb0 - b0).abs() < 1e-10 && (nb1 - b1).abs() < 1e-10 {
                b0 = nb0;
                b1 = nb1;
                converged = true;
                break;
            }
            b0 = nb0;
            b1 = nb1;
        }
        let mut dev = 0.0;
        for i in 0..n {
            let mu = (b0 * x0s[i] + b1 * x1s[i]).exp().max(1e-12);
            let yi = ys[i];
            if yi > 0.0 {
                dev += yi * (yi / mu).ln() - (yi - mu);
            } else {
                dev += mu;
            }
        }
        dev *= 2.0;
        let coef = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _c = protect(coef);
        *REAL(coef) = b0;
        *REAL(coef).add(1) = b1;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, coef);
        SET_VECTOR_ELT(result, 1, Rf_ScalarLogical(if converged { 1 } else { 0 }));
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(dev));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "coefficients".to_string(),
                "converged".to_string(),
                "deviance".to_string(),
            ],
        );
        result
    }
}

/// GNU `glm.control(epsilon=1e-8, maxit=25, trace=FALSE)`.
pub unsafe fn do_glm_control(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut epsilon = 1e-8;
        let mut maxit = 25.0;
        let mut trace = 0;
        let mut tol = f64::NAN;
        let mut wtol = 0.0;
        let mut tol_set = false;
        let mut cell = args;
        let mut pos = 0usize;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let v = CAR(cell);
            if name == "epsilon" || (name.is_empty() && pos == 0) {
                if !v.is_null() && v != R_NilValue() {
                    epsilon = elt_real_safe(v, 0);
                }
            } else if name == "maxit" || (name.is_empty() && pos == 1) {
                if !v.is_null() && v != R_NilValue() {
                    maxit = elt_real_safe(v, 0);
                }
            } else if name == "trace" || (name.is_empty() && pos == 2) {
                if TYPEOF(v) == SEXPTYPE::LGLSXP {
                    trace = *LOGICAL(v);
                }
            } else if name == "tol" || (name.is_empty() && pos == 3) {
                if !v.is_null() && v != R_NilValue() {
                    tol = elt_real_safe(v, 0);
                    tol_set = true;
                }
            } else if name == "wtol" || (name.is_empty() && pos == 4) {
                if !v.is_null() && v != R_NilValue() {
                    wtol = elt_real_safe(v, 0);
                }
            }
            pos += 1;
            cell = CDR(cell);
        }
        if epsilon.is_nan() || epsilon <= 0.0 {
            crate::mainutils::essentials::base_error("value of 'epsilon' must be > 0".to_owned());
        }
        if maxit.is_nan() || maxit <= 0.0 {
            crate::mainutils::essentials::base_error(
                "maximum number of iterations must be > 0".to_owned(),
            );
        }
        if !tol_set {
            tol = (1e-7_f64).min(epsilon / 1000.0);
        }
        if tol.is_nan() || tol < 0.0 {
            crate::mainutils::essentials::base_error("value of 'tol' must be >= 0".to_owned());
        }
        if wtol.is_nan() || wtol < 0.0 {
            crate::mainutils::essentials::base_error("value of 'wtol' must be >= 0".to_owned());
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(epsilon));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(maxit));
        SET_VECTOR_ELT(result, 2, Rf_ScalarLogical(trace));
        SET_VECTOR_ELT(result, 3, Rf_ScalarReal(tol));
        SET_VECTOR_ELT(result, 4, Rf_ScalarReal(wtol));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "epsilon".to_string(),
                "maxit".to_string(),
                "trace".to_string(),
                "tol".to_string(),
                "wtol".to_string(),
            ],
        );
        result
    }
}

/// GNU `nls.control(maxiter=50, tol=1e-5, minFactor=1/1024)`.
pub unsafe fn do_nls_control(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut maxiter = 50.0;
        let mut tol = 1e-5;
        let mut min_factor = 1.0 / 1024.0;
        let mut cell = args;
        let mut pos = 0usize;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let v = CAR(cell);
            if name == "maxiter" || (name.is_empty() && pos == 0) {
                if !v.is_null() && v != R_NilValue() {
                    maxiter = elt_real_safe(v, 0);
                }
            } else if name == "tol" || (name.is_empty() && pos == 1) {
                if !v.is_null() && v != R_NilValue() {
                    tol = elt_real_safe(v, 0);
                }
            } else if name == "minFactor" || (name.is_empty() && pos == 2) {
                if !v.is_null() && v != R_NilValue() {
                    min_factor = elt_real_safe(v, 0);
                }
            }
            pos += 1;
            cell = CDR(cell);
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(maxiter));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(tol));
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(min_factor));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "maxiter".to_string(),
                "tol".to_string(),
                "minFactor".to_string(),
            ],
        );
        result
    }
}

/// GNU `nls(y ~ expr, start=)` — one-parameter Gauss–Newton.
pub unsafe fn do_nls(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let form = CAR(args);
        if form.is_null() || form == R_NilValue() || TYPEOF(form) != SEXPTYPE::LANGSXP {
            return R_NilValue();
        }
        let y_expr = CADR(form);
        let rhs = CAR(CDR(CDR(form)));
        let y = crate::eval::eval::Rf_eval(y_expr, rho);
        let _y = protect(y);
        let n = XLENGTH(y) as usize;
        if n == 0 {
            return R_NilValue();
        }
        let mut start = std::ptr::null_mut();
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "start" || (name.is_empty() && TYPEOF(CAR(cell)) == SEXPTYPE::VECSXP) {
                start = CAR(cell);
                break;
            }
            cell = CDR(cell);
        }
        if start.is_null() || TYPEOF(start) != SEXPTYPE::VECSXP || XLENGTH(start) < 1 {
            return R_NilValue();
        }
        let names =
            crate::sexp::attrib_core::getAttrib(start, crate::sexp::attrib_core::R_NamesSymbol());
        if TYPEOF(names) != SEXPTYPE::STRSXP || XLENGTH(names) < 1 {
            return R_NilValue();
        }
        let pname = STRING_ELT(names, 0);
        let psym = Rf_install(CHAR(pname));
        let mut b = elt_real_safe(VECTOR_ELT(start, 0), 0);
        let mut ys = vec![0.0; n];
        for i in 0..n {
            ys[i] = elt_real_safe(y, i as i64);
        }
        for _ in 0..30 {
            crate::sexp::envir::defineVar(psym, Rf_ScalarReal(b), rho);
            let pred = crate::eval::eval::Rf_eval(rhs, rho);
            let _p = protect(pred);
            let eps = 1e-6 * (b.abs() + 1.0);
            crate::sexp::envir::defineVar(psym, Rf_ScalarReal(b + eps), rho);
            let pred2 = crate::eval::eval::Rf_eval(rhs, rho);
            let _p2 = protect(pred2);
            let mut jtj = 0.0;
            let mut jtr = 0.0;
            for i in 0..n {
                let f = elt_real_safe(pred, i as i64);
                let f2 = elt_real_safe(pred2, i as i64);
                let ji = (f2 - f) / eps;
                jtj += ji * ji;
                jtr += ji * (ys[i] - f);
            }
            if jtj <= 0.0 {
                break;
            }
            let db = jtr / jtj;
            b += db;
            if db.abs() < 1e-12 {
                break;
            }
        }
        crate::sexp::envir::defineVar(psym, Rf_ScalarReal(b), rho);
        let coef = Rf_ScalarReal(b);
        let _c = protect(coef);
        let pname_str = std::ffi::CStr::from_ptr(CHAR(pname))
            .to_string_lossy()
            .into_owned();
        crate::mainutils::essentials::set_string_names(coef, &[pname_str]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, coef);
        crate::mainutils::essentials::set_string_names(result, &["coefficients".to_string()]);
        let class = Rf_mkString(c"nls".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `SSlogis(input, Asym, xmid, scal)` — logistic curve.
pub unsafe fn do_sslogis(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let input = CAR(args);
        let asym = elt_real_safe(CAR(CDR(args)), 0);
        let xmid = elt_real_safe(CAR(CDR(CDR(args))), 0);
        let scal = elt_real_safe(CAR(CDR(CDR(CDR(args)))), 0);
        if scal == 0.0 {
            return R_NilValue();
        }
        let n = XLENGTH(input);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let x = elt_real_safe(input, i);
            *REAL(result).add(i as usize) = asym / (1.0 + ((xmid - x) / scal).exp());
        }
        result
    }
}

/// GNU `SSasymp(input, Asym, R0, lrc)` — asymptotic exponential.
pub unsafe fn do_ssasymp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let input = CAR(args);
        let asym = elt_real_safe(CAR(CDR(args)), 0);
        let r0 = elt_real_safe(CAR(CDR(CDR(args))), 0);
        let lrc = elt_real_safe(CAR(CDR(CDR(CDR(args)))), 0);
        let rc = lrc.exp();
        let n = XLENGTH(input);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let x = elt_real_safe(input, i);
            *REAL(result).add(i as usize) = asym + (r0 - asym) * (-rc * x).exp();
        }
        result
    }
}

/// GNU `SSmicmen(input, Vm, K)` — Michaelis–Menten.
pub unsafe fn do_ssmicmen(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let input = CAR(args);
        let vm = elt_real_safe(CAR(CDR(args)), 0);
        let k = elt_real_safe(CAR(CDR(CDR(args))), 0);
        let n = XLENGTH(input);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let x = elt_real_safe(input, i);
            *REAL(result).add(i as usize) = vm * x / (k + x);
        }
        result
    }
}

/// GNU `SSgompertz(input, Asym, b2, b3)` — Gompertz growth.
pub unsafe fn do_ssgompertz(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let input = CAR(args);
        let asym = elt_real_safe(CAR(CDR(args)), 0);
        let b2 = elt_real_safe(CAR(CDR(CDR(args))), 0);
        let b3 = elt_real_safe(CAR(CDR(CDR(CDR(args)))), 0);
        let n = XLENGTH(input);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let x = elt_real_safe(input, i);
            *REAL(result).add(i as usize) = asym * (-b2 * b3.powf(x)).exp();
        }
        result
    }
}

/// GNU `SSbiexp(input, A1, lrc1, A2, lrc2)` — biexponential decay.
pub unsafe fn do_ssbiexp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let input = CAR(args);
        let a1 = elt_real_safe(CAR(CDR(args)), 0);
        let lrc1 = elt_real_safe(CAR(CDR(CDR(args))), 0);
        let a2 = elt_real_safe(CAR(CDR(CDR(CDR(args)))), 0);
        let lrc2 = elt_real_safe(CAR(CDR(CDR(CDR(CDR(args))))), 0);
        let n = XLENGTH(input);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let x = elt_real_safe(input, i);
            *REAL(result).add(i as usize) =
                a1 * (-lrc1.exp() * x).exp() + a2 * (-lrc2.exp() * x).exp();
        }
        result
    }
}

/// GNU `SSasympOff(input, Asym, lrc, c0)` — asymptotic with offset.
pub unsafe fn do_ssasymp_off(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let input = CAR(args);
        let asym = elt_real_safe(CAR(CDR(args)), 0);
        let lrc = elt_real_safe(CAR(CDR(CDR(args))), 0);
        let c0 = elt_real_safe(CAR(CDR(CDR(CDR(args)))), 0);
        let rc = lrc.exp();
        let n = XLENGTH(input);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let x = elt_real_safe(input, i);
            *REAL(result).add(i as usize) = asym * (1.0 - (-rc * (x - c0)).exp());
        }
        result
    }
}

/// GNU `SSasympOrig(input, Asym, lrc)` — asymptotic through the origin.
pub unsafe fn do_ssasymp_orig(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let input = CAR(args);
        let asym = elt_real_safe(CAR(CDR(args)), 0);
        let lrc = elt_real_safe(CAR(CDR(CDR(args))), 0);
        let rc = lrc.exp();
        let n = XLENGTH(input);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let x = elt_real_safe(input, i);
            *REAL(result).add(i as usize) = asym * (1.0 - (-rc * x).exp());
        }
        result
    }
}

/// GNU `SSfol(Dose, input, lKe, lKa, lCl)` — first-order compartment.
pub unsafe fn do_ssfol(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let dose = elt_real_safe(CAR(args), 0);
        let input = CAR(CDR(args));
        let lke = elt_real_safe(CAR(CDR(CDR(args))), 0);
        let lka = elt_real_safe(CAR(CDR(CDR(CDR(args)))), 0);
        let lcl = elt_real_safe(CAR(CDR(CDR(CDR(CDR(args))))), 0);
        let ke = lke.exp();
        let ka = lka.exp();
        let scale = dose * (lke + lka - lcl).exp() / (ka - ke);
        let n = XLENGTH(input);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let t = elt_real_safe(input, i);
            *REAL(result).add(i as usize) = scale * ((-ke * t).exp() - (-ka * t).exp());
        }
        result
    }
}

/// GNU `SSweibull(x, Asym, Drop, lrc, pwr)` — Weibull growth.
pub unsafe fn do_ssweibull(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let input = CAR(args);
        let asym = elt_real_safe(CAR(CDR(args)), 0);
        let drop = elt_real_safe(CAR(CDR(CDR(args))), 0);
        let lrc = elt_real_safe(CAR(CDR(CDR(CDR(args)))), 0);
        let pwr = elt_real_safe(CAR(CDR(CDR(CDR(CDR(args))))), 0);
        let rc = lrc.exp();
        let n = XLENGTH(input);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let x = elt_real_safe(input, i);
            *REAL(result).add(i as usize) = asym - drop * (-rc * x.powf(pwr)).exp();
        }
        result
    }
}

/// GNU `SSfpl(input, A, B, xmid, scal)` — four-parameter logistic.
pub unsafe fn do_ssfpl(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let input = CAR(args);
        let a = elt_real_safe(CAR(CDR(args)), 0);
        let b = elt_real_safe(CAR(CDR(CDR(args))), 0);
        let xmid = elt_real_safe(CAR(CDR(CDR(CDR(args)))), 0);
        let scal = elt_real_safe(CAR(CDR(CDR(CDR(CDR(args))))), 0);
        if scal == 0.0 {
            return R_NilValue();
        }
        let n = XLENGTH(input);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let x = elt_real_safe(input, i);
            *REAL(result).add(i as usize) = a + (b - a) / (1.0 + ((xmid - x) / scal).exp());
        }
        result
    }
}
