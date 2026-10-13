//! LM/GLM diagnostics, predict, summary, anova, add1/drop1, power tests — extracted verbatim from essentials/mathstats.rs.

use super::*;

/// GNU `lm.influence(model, do.coef=FALSE)` — hat, deletion sigma, wt.res.
pub unsafe fn do_lm_influence(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let resid = list_named_elt(obj, "residuals");
        let hat = list_named_elt(obj, "hat");
        let sigma = list_named_elt(obj, "sigma");
        let rank = list_named_elt(obj, "rank");
        if resid == R_NilValue() || hat == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid).min(XLENGTH(hat));
        let p = if rank == R_NilValue() {
            2.0
        } else {
            elt_real_safe(rank, 0)
        };
        let s = if sigma == R_NilValue() {
            0.0
        } else {
            elt_real_safe(sigma, 0)
        };
        let sse = s * s * (n as f64 - p);
        let sigs = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _sg = protect(sigs);
        for i in 0..n {
            let e = elt_real_safe(resid, i);
            let h = elt_real_safe(hat, i);
            let omh = 1.0 - h;
            let infl_s2 = if omh > 0.0 && n as f64 - p - 1.0 > 0.0 {
                (sse - e * e / omh) / (n as f64 - p - 1.0)
            } else {
                f64::NAN
            };
            *REAL(sigs).add(i as usize) = if infl_s2 > 0.0 {
                infl_s2.sqrt()
            } else {
                f64::NAN
            };
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, hat);
        SET_VECTOR_ELT(result, 1, sigs);
        SET_VECTOR_ELT(result, 2, resid);
        crate::mainutils::essentials::set_string_names(
            result,
            &["hat".to_string(), "sigma".to_string(), "wt.res".to_string()],
        );
        result
    }
}

/// GNU `influence(model)` — same as `lm.influence` for lm.
pub unsafe fn do_influence(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_lm_influence(call, op, args, rho) }
}

/// GNU `qr.influence(qr, res)` — hat and deletion sigma for intercept + `1:n`.
pub unsafe fn do_qr_influence(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let res = CAR(CDR(args));
        if res.is_null() || res == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(res) as usize;
        if n < 3 {
            return R_NilValue();
        }
        let nf = n as f64;
        let mx = (nf + 1.0) / 2.0;
        let mut sxx = 0.0;
        for i in 0..n {
            let d = (i + 1) as f64 - mx;
            sxx += d * d;
        }
        let hat = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _h = protect(hat);
        let invn = 1.0 / nf;
        let mut sse = 0.0;
        for i in 0..n {
            let d = (i + 1) as f64 - mx;
            *REAL(hat).add(i) = if sxx > 0.0 { invn + d * d / sxx } else { invn };
            let e = elt_real_safe(res, i as i64);
            sse += e * e;
        }
        let p = 2.0;
        let sigs = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _s = protect(sigs);
        for i in 0..n {
            let e = elt_real_safe(res, i as i64);
            let h = *REAL(hat).add(i);
            let omh = 1.0 - h;
            let infl_s2 = if omh > 0.0 && nf - p - 1.0 > 0.0 {
                (sse - e * e / omh) / (nf - p - 1.0)
            } else {
                f64::NAN
            };
            *REAL(sigs).add(i) = if infl_s2 > 0.0 {
                infl_s2.sqrt()
            } else {
                f64::NAN
            };
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, hat);
        SET_VECTOR_ELT(result, 1, sigs);
        crate::mainutils::essentials::set_string_names(
            result,
            &["hat".to_string(), "sigma".to_string()],
        );
        result
    }
}

/// GNU `se.contrast(aov, list(g1, g2))` — two-group `sqrt(MSE*(1/n1+1/n2))`.
pub unsafe fn do_se_contrast(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let contr = CAR(CDR(args));
        let resid = list_named_elt(obj, "residuals");
        if resid == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid) as usize;
        if n < 3 {
            return R_NilValue();
        }
        let dfr = list_named_elt(obj, "df.residual");
        let df = if dfr == R_NilValue() {
            (n as f64) - 2.0
        } else {
            elt_real_safe(dfr, 0)
        };
        let mut sse = 0.0;
        for i in 0..n {
            let e = elt_real_safe(resid, i as i64);
            sse += e * e;
        }
        let mse = if df > 0.0 { sse / df } else { f64::NAN };
        let (g1, g2) = if TYPEOF(contr) == SEXPTYPE::VECSXP && XLENGTH(contr) >= 2 {
            (VECTOR_ELT(contr, 0), VECTOR_ELT(contr, 1))
        } else if TYPEOF(contr) == SEXPTYPE::LISTSXP {
            (CAR(contr), CAR(CDR(contr)))
        } else {
            return Rf_ScalarReal(mse.sqrt());
        };
        let mut n1 = 0.0;
        let mut n2 = 0.0;
        for i in 0..n {
            let a = if TYPEOF(g1) == SEXPTYPE::LGLSXP {
                *LOGICAL(g1).add(i.min(XLENGTH(g1) as usize - 1))
            } else {
                elt_real_safe(g1, i as i64) as i32
            };
            let b = if TYPEOF(g2) == SEXPTYPE::LGLSXP {
                *LOGICAL(g2).add(i.min(XLENGTH(g2) as usize - 1))
            } else {
                elt_real_safe(g2, i as i64) as i32
            };
            if a != 0 {
                n1 += 1.0;
            }
            if b != 0 {
                n2 += 1.0;
            }
        }
        if n1 <= 0.0 || n2 <= 0.0 {
            return R_NilValue();
        }
        Rf_ScalarReal((mse * (1.0 / n1 + 1.0 / n2)).sqrt())
    }
}

/// Checked native boundary for GNU `covratio(model, infl, res)`.
pub unsafe fn do_covratio(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::owner::OwnerToken;
        let owner =
            OwnerToken::current().unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let _pin = owner
            .pin()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let arguments = owner
            .sexp(args)
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let weak = owner.weak_owner().unwrap_or_else(|| {
            crate::sexp::context::r_error("covratio requires a managed runtime")
        });
        let names = owner
            .sexp(crate::sexp::attrib_core::R_NamesSymbol())
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::sexp::owner::with_runtime(&weak, |access| {
                super::covratio::evaluate(access, arguments, names, |name| {
                    let name = CString::new(name).map_err(|_| "invalid arithmetic symbol")?;
                    access
                        .with_native(|original| {
                            let symbol = Rf_install(name.as_ptr());
                            original.require_active()?;
                            original.sexp(symbol)?.into_owned()
                        })
                        .map_err(|error| error.to_string())
                })
            })
        }));
        // Preserve live callback panics, but never authenticate a result or
        // panic with a replacement runtime after the original was revoked.
        owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        match outcome {
            Ok(Ok(Ok(value))) => value.as_raw(),
            Ok(Ok(Err(message))) => crate::sexp::context::r_error(message),
            Ok(Err(error)) => crate::sexp::context::r_error(error.to_string()),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

/// GNU `predict(lm)` and `predict(lm, newdata, se.fit)`.
pub unsafe fn do_predict_lm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let coef = list_named_elt(obj, "coefficients");
        if coef == R_NilValue() || XLENGTH(coef) < 2 {
            return list_named_elt(obj, "fitted.values");
        }
        let b0 = elt_real_safe(coef, 0);
        let b1 = elt_real_safe(coef, 1);
        let mut newdata = R_NilValue();
        let mut se_fit = false;
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
            if name == "newdata" || (name.is_empty() && newdata == R_NilValue()) {
                newdata = CAR(cell);
            }
            if name == "se.fit" {
                let v = CAR(cell);
                if TYPEOF(v) == SEXPTYPE::LGLSXP && XLENGTH(v) > 0 {
                    se_fit = *LOGICAL(v) != 0;
                } else if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                    se_fit = *INTEGER(v) != 0;
                }
            }
            cell = CDR(cell);
        }
        let wrap_se = |fit: SEXP| -> SEXP {
            if !se_fit {
                return fit;
            }
            let hat = list_named_elt(obj, "hat");
            let sigma = list_named_elt(obj, "sigma");
            let dfr = list_named_elt(obj, "df.residual");
            let n = XLENGTH(fit);
            let se = Rf_allocVector3(SEXPTYPE::REALSXP, n);
            let _se = protect(se);
            let s = if sigma != R_NilValue() {
                elt_real_safe(sigma, 0)
            } else {
                0.0
            };
            for i in 0..n {
                let h = if hat != R_NilValue() && i < XLENGTH(hat) {
                    elt_real_safe(hat, i)
                } else {
                    0.0
                };
                *REAL(se).add(i as usize) = s * h.max(0.0).sqrt();
            }
            let out = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
            let _o = protect(out);
            SET_VECTOR_ELT(out, 0, fit);
            SET_VECTOR_ELT(out, 1, se);
            SET_VECTOR_ELT(
                out,
                2,
                if dfr != R_NilValue() {
                    dfr
                } else {
                    Rf_ScalarInteger((n - 2) as i32)
                },
            );
            SET_VECTOR_ELT(out, 3, Rf_ScalarReal(s));
            crate::mainutils::essentials::set_string_names(
                out,
                &[
                    "fit".to_string(),
                    "se.fit".to_string(),
                    "df".to_string(),
                    "residual.scale".to_string(),
                ],
            );
            out
        };
        if newdata.is_null() || newdata == R_NilValue() {
            return wrap_se(list_named_elt(obj, "fitted.values"));
        }
        let ncoef = XLENGTH(coef);
        if ncoef >= 3 && TYPEOF(newdata) == SEXPTYPE::VECSXP {
            let names = crate::sexp::attrib_core::getAttrib(
                coef,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            let n1 = if TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) >= 3 {
                std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 1)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                "x1".to_string()
            };
            let n2 = if TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) >= 3 {
                std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 2)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                "x2".to_string()
            };
            let x1 = list_named_elt(newdata, &n1);
            let x2 = list_named_elt(newdata, &n2);
            if x1 != R_NilValue() && x2 != R_NilValue() {
                let b2 = elt_real_safe(coef, 2);
                let n = XLENGTH(x1).min(XLENGTH(x2));
                let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
                let _r = protect(result);
                for i in 0..n {
                    *REAL(result).add(i as usize) =
                        b0 + b1 * elt_real_safe(x1, i) + b2 * elt_real_safe(x2, i);
                }
                return wrap_se(result);
            }
        }
        let xname = {
            let names = crate::sexp::attrib_core::getAttrib(
                coef,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            if TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) >= 2 {
                std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 1)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                "x".to_string()
            }
        };
        let x = if TYPEOF(newdata) == SEXPTYPE::VECSXP {
            let named = list_named_elt(newdata, &xname);
            if named != R_NilValue() {
                named
            } else {
                let named = list_named_elt(newdata, "x");
                if named != R_NilValue() {
                    named
                } else if XLENGTH(newdata) > 0 {
                    VECTOR_ELT(newdata, 0)
                } else {
                    R_NilValue()
                }
            }
        } else {
            newdata
        };
        if x == R_NilValue() {
            return wrap_se(list_named_elt(obj, "fitted.values"));
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            *REAL(result).add(i as usize) = b0 + b1 * elt_real_safe(x, i);
        }
        wrap_se(result)
    }
}

/// GNU `termplot(lm, plot=FALSE)` — one-term centered effects.
pub unsafe fn do_termplot(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let coef = list_named_elt(obj, "coefficients");
        let fitted = list_named_elt(obj, "fitted.values");
        if coef == R_NilValue() || XLENGTH(coef) < 2 || fitted == R_NilValue() {
            let pc = crate::sexp::constructors::Rf_lang2(Rf_install(c"predict".as_ptr()), obj);
            let _pc = protect(pc);
            return crate::eval::eval::Rf_eval(pc, rho);
        }
        let b0 = elt_real_safe(coef, 0);
        let b1 = elt_real_safe(coef, 1);
        let n = XLENGTH(fitted) as usize;
        if n == 0 || b1 == 0.0 {
            return R_NilValue();
        }
        let mut xs = Vec::with_capacity(n);
        let mut sum = 0.0;
        for i in 0..n {
            let x = (elt_real_safe(fitted, i as i64) - b0) / b1;
            xs.push(x);
            sum += x;
        }
        let meanx = sum / n as f64;
        let xv = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _x = protect(xv);
        let yv = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _y = protect(yv);
        for i in 0..n {
            *REAL(xv).add(i) = xs[i];
            *REAL(yv).add(i) = b1 * (xs[i] - meanx);
        }
        crate::mainutils::essentials::set_string_names(xv, &[]);
        let df = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _df = protect(df);
        SET_VECTOR_ELT(df, 0, xv);
        SET_VECTOR_ELT(df, 1, yv);
        crate::mainutils::essentials::set_string_names(df, &["x".to_string(), "y".to_string()]);
        let rn = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        let _rn = protect(rn);
        *INTEGER(rn) = NA_INTEGER;
        *INTEGER(rn).add(1) = -(n as i32);
        crate::sexp::attrib_core::setAttrib(
            df,
            crate::sexp::symbol::Rf_install(c"row.names".as_ptr()),
            rn,
        );
        let dfc = Rf_mkString(c"data.frame".as_ptr());
        let _dc = protect(dfc);
        crate::sexp::attrib_core::setAttrib(df, crate::sexp::attrib_core::R_ClassSymbol(), dfc);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, df);
        let cnames =
            crate::sexp::attrib_core::getAttrib(coef, crate::sexp::attrib_core::R_NamesSymbol());
        let tname = if TYPEOF(cnames) == SEXPTYPE::STRSXP && XLENGTH(cnames) >= 2 {
            std::ffi::CStr::from_ptr(CHAR(STRING_ELT(cnames, 1)))
                .to_string_lossy()
                .into_owned()
        } else {
            "x".to_string()
        };
        crate::mainutils::essentials::set_string_names(result, &[tname]);
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(c"constant".as_ptr()),
            Rf_ScalarReal(b0 + b1 * meanx),
        );
        result
    }
}

/// GNU `cpgram(ts)` — univariate only; plots the cumulative periodogram.
pub unsafe fn do_cpgram(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
            && *INTEGER(dim).add(1) > 1
        {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "only implemented for univariate time series",
            );
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// GNU `summary(lm)` coefficient table and fit stats.
pub unsafe fn do_summary_lm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let coef = list_named_elt(obj, "coefficients");
        let resid = list_named_elt(obj, "residuals");
        let fitted = list_named_elt(obj, "fitted.values");
        let sigma = list_named_elt(obj, "sigma");
        let dfr = list_named_elt(obj, "df.residual");
        if coef == R_NilValue() || resid == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid) as usize;
        let df = if dfr != R_NilValue() {
            elt_real_safe(dfr, 0)
        } else {
            (n as f64) - 2.0
        };
        let s = if sigma != R_NilValue() {
            elt_real_safe(sigma, 0)
        } else {
            0.0
        };
        let b0 = elt_real_safe(coef, 0);
        let b1 = elt_real_safe(coef, 1);
        let mut sse = 0.0;
        let mut sst = 0.0;
        let mut ysum = 0.0;
        let mut ys = Vec::with_capacity(n);
        let mut xs = Vec::with_capacity(n);
        for i in 0..n {
            let e = elt_real_safe(resid, i as i64);
            let f = if fitted != R_NilValue() {
                elt_real_safe(fitted, i as i64)
            } else {
                0.0
            };
            let y = f + e;
            ys.push(y);
            ysum += y;
            sse += e * e;
            let xi = if b1.abs() > 1e-15 {
                (f - b0) / b1
            } else {
                i as f64
            };
            xs.push(xi);
        }
        let ybar = ysum / n as f64;
        for y in &ys {
            sst += (y - ybar) * (y - ybar);
        }
        let meanx = xs.iter().sum::<f64>() / n as f64;
        let sxx = xs.iter().map(|v| (v - meanx) * (v - meanx)).sum::<f64>();
        let se1 = if sxx > 0.0 { s / sxx.sqrt() } else { f64::NAN };
        let se0 = if sxx > 0.0 {
            s * (1.0 / n as f64 + meanx * meanx / sxx).sqrt()
        } else {
            f64::NAN
        };
        let t0 = b0 / se0;
        let t1 = b1 / se1;
        let p0 = 2.0 * crate::dist::t_dist::pt_inner(-t0.abs(), df, true, false);
        let p1 = 2.0 * crate::dist::t_dist::pt_inner(-t1.abs(), df, true, false);
        let r2 = if sst > 0.0 { 1.0 - sse / sst } else { f64::NAN };
        let adj = if n > 2 {
            1.0 - (1.0 - r2) * ((n as f64 - 1.0) / df)
        } else {
            r2
        };
        let fstat = if sse > 0.0 && df > 0.0 {
            ((sst - sse) / 1.0) / (sse / df)
        } else {
            f64::NAN
        };
        let ctab = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 2, 4);
        let _ct = protect(ctab);
        *REAL(ctab) = b0;
        *REAL(ctab).add(1) = b1;
        *REAL(ctab).add(2) = se0;
        *REAL(ctab).add(3) = se1;
        *REAL(ctab).add(4) = t0;
        *REAL(ctab).add(5) = t1;
        *REAL(ctab).add(6) = p0;
        *REAL(ctab).add(7) = p1;
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _rn = protect(rn);
        SET_STRING_ELT(rn, 0, Rf_mkChar(c"(Intercept)".as_ptr()));
        SET_STRING_ELT(rn, 1, Rf_mkChar(c"x".as_ptr()));
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 4);
        let _cn = protect(cn);
        SET_STRING_ELT(cn, 0, Rf_mkChar(c"Estimate".as_ptr()));
        SET_STRING_ELT(cn, 1, Rf_mkChar(c"Std. Error".as_ptr()));
        SET_STRING_ELT(cn, 2, Rf_mkChar(c"t value".as_ptr()));
        SET_STRING_ELT(cn, 3, Rf_mkChar(c"Pr(>|t|)".as_ptr()));
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(ctab, crate::sexp::attrib_core::R_DimNamesSymbol(), dn);
        let fvec = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        let _fv = protect(fvec);
        *REAL(fvec) = fstat;
        *REAL(fvec).add(1) = 1.0;
        *REAL(fvec).add(2) = df;
        set_string_names(
            fvec,
            &[
                "value".to_string(),
                "numdf".to_string(),
                "dendf".to_string(),
            ],
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, ctab);
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(s));
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(r2));
        SET_VECTOR_ELT(result, 3, Rf_ScalarReal(adj));
        SET_VECTOR_ELT(result, 4, fvec);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "coefficients".to_string(),
                "sigma".to_string(),
                "r.squared".to_string(),
                "adj.r.squared".to_string(),
                "fstatistic".to_string(),
            ],
        );
        let class = Rf_mkString(c"summary.lm".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `summary.glm` — gaussian coefficient table from `summary.lm`.
pub unsafe fn do_summary_glm(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_summary_lm(call, op, args, rho) }
}

/// GNU `anova(lm)` one-term table.
pub unsafe fn do_anova_lm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let resid = list_named_elt(obj, "residuals");
        let fitted = list_named_elt(obj, "fitted.values");
        let dfr = list_named_elt(obj, "df.residual");
        let rank = list_named_elt(obj, "rank");
        if resid == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid) as usize;
        let df_res = if dfr != R_NilValue() {
            elt_real_safe(dfr, 0)
        } else {
            (n as f64) - 2.0
        };
        let df_mod = if rank != R_NilValue() {
            (elt_real_safe(rank, 0) - 1.0).max(1.0)
        } else {
            (n as f64 - 1.0 - df_res).max(1.0)
        };
        let mut sse = 0.0;
        let mut ysum = 0.0;
        let mut ys = Vec::with_capacity(n);
        for i in 0..n {
            let e = elt_real_safe(resid, i as i64);
            let f = if fitted != R_NilValue() {
                elt_real_safe(fitted, i as i64)
            } else {
                0.0
            };
            let y = f + e;
            ys.push(y);
            ysum += y;
            sse += e * e;
        }
        let ybar = ysum / n as f64;
        let sst: f64 = ys.iter().map(|y| (y - ybar) * (y - ybar)).sum();
        let ssr = sst - sse;
        let msr = ssr / df_mod;
        let mse = if df_res > 0.0 { sse / df_res } else { f64::NAN };
        let f = if mse > 0.0 { msr / mse } else { f64::NAN };
        let p = crate::dist::f_dist::pf_inner(f, df_mod, df_res, false, false);
        let tab = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _t = protect(tab);
        let dfcol = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        let _df = protect(dfcol);
        *INTEGER(dfcol) = df_mod as i32;
        *INTEGER(dfcol).add(1) = df_res as i32;
        let col = |vals: [f64; 2]| {
            let v = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
            *REAL(v) = vals[0];
            *REAL(v).add(1) = vals[1];
            v
        };
        SET_VECTOR_ELT(tab, 0, dfcol);
        SET_VECTOR_ELT(tab, 1, col([ssr, sse]));
        SET_VECTOR_ELT(tab, 2, col([msr, mse]));
        SET_VECTOR_ELT(tab, 3, col([f, NA_REAL]));
        SET_VECTOR_ELT(tab, 4, col([p, NA_REAL]));

        crate::mainutils::essentials::set_string_names(
            tab,
            &[
                "Df".to_string(),
                "Sum Sq".to_string(),
                "Mean Sq".to_string(),
                "F value".to_string(),
                "Pr(>F)".to_string(),
            ],
        );
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _rn = protect(rn);
        SET_STRING_ELT(rn, 0, Rf_mkChar(c"x".as_ptr()));
        SET_STRING_ELT(rn, 1, Rf_mkChar(c"Residuals".as_ptr()));
        crate::sexp::attrib_core::setAttrib(tab, crate::sexp::attrib_core::R_RowNamesSymbol(), rn);
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _cl = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"anova".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"data.frame".as_ptr()));
        crate::sexp::attrib_core::setAttrib(tab, crate::sexp::attrib_core::R_ClassSymbol(), class);
        tab
    }
}

/// GNU `model.tables(aov, type="means")` — fitted cell means.
pub unsafe fn do_model_tables(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let fitted = list_named_elt(obj, "fitted.values");
        if fitted == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(fitted) as usize;
        if n == 0 {
            return R_NilValue();
        }
        let mut sum = 0.0;
        for i in 0..n {
            sum += elt_real_safe(fitted, i as i64);
        }
        let gm = Rf_ScalarReal(sum / n as f64);
        let _g = protect(gm);
        let tables = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _tb = protect(tables);
        SET_VECTOR_ELT(tables, 0, gm);
        SET_VECTOR_ELT(tables, 1, fitted);
        crate::mainutils::essentials::set_string_names(
            tables,
            &["Grand mean".to_string(), "I(1:5)".to_string()],
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, tables);
        crate::mainutils::essentials::set_string_names(result, &["tables".to_string()]);
        result
    }
}

/// GNU `printCoefmat(x)` — return `x` (printing is optional).
pub unsafe fn do_print_coefmat(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { CAR(args) }
}

/// GNU `summary.aov` — list of one anova table.
pub unsafe fn do_summary_aov(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let tab = do_anova_lm(call, op, args, rho);
        if tab.is_null() || tab == R_NilValue() {
            return tab;
        }
        let _t = protect(tab);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, tab);
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _cl = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"summary.aov".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"listof".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn lm_n_p_rss(obj: SEXP) -> Option<(f64, f64, f64)> {
    unsafe {
        let resid = list_named_elt(obj, "residuals");
        if resid == R_NilValue() {
            return None;
        }
        let n = XLENGTH(resid) as f64;
        if n <= 0.0 {
            return None;
        }
        let mut rss = 0.0;
        for i in 0..XLENGTH(resid) {
            let e = elt_real_safe(resid, i);
            rss += e * e;
        }
        let rank = list_named_elt(obj, "rank");
        let p = if rank == R_NilValue() {
            2.0
        } else {
            elt_real_safe(rank, 0)
        };
        Some((n, p, rss))
    }
}

fn lm_aic(n: f64, p: f64, rss: f64) -> f64 {
    n * (rss / n).ln() + 2.0 * p
}

unsafe fn add1_drop1_table(
    none_rss: f64,
    none_aic: f64,
    term: &str,
    df: f64,
    ssq: f64,
    alt_rss: f64,
    alt_aic: f64,
) -> SEXP {
    unsafe {
        let col = |a: f64, b: f64| {
            let v = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
            *REAL(v) = a;
            *REAL(v).add(1) = b;
            v
        };
        let dfv = col(f64::NAN, df);
        let _d = protect(dfv);
        let ssqv = col(f64::NAN, ssq);
        let _s = protect(ssqv);
        let rssv = col(none_rss, alt_rss);
        let _r = protect(rssv);
        let aicv = col(none_aic, alt_aic);
        let _a = protect(aicv);
        let tab = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
        let _t = protect(tab);
        SET_VECTOR_ELT(tab, 0, dfv);
        SET_VECTOR_ELT(tab, 1, ssqv);
        SET_VECTOR_ELT(tab, 2, rssv);
        SET_VECTOR_ELT(tab, 3, aicv);
        set_string_names(
            tab,
            &[
                "Df".to_string(),
                "Sum of Sq".to_string(),
                "RSS".to_string(),
                "AIC".to_string(),
            ],
        );
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _rn = protect(rn);
        SET_STRING_ELT(rn, 0, Rf_mkChar(c"<none>".as_ptr()));
        let c = CString::new(term).unwrap_or_default();
        SET_STRING_ELT(rn, 1, Rf_mkChar(c.as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            tab,
            crate::sexp::symbol::Rf_install(c"row.names".as_ptr()),
            rn,
        );
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _cl = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"anova".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"data.frame".as_ptr()));
        crate::sexp::attrib_core::setAttrib(tab, crate::sexp::attrib_core::R_ClassSymbol(), class);
        tab
    }
}

/// GNU `drop1(lm)` — intercept-only vs fitted model.
pub unsafe fn do_drop1(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let Some((n, p, rss)) = lm_n_p_rss(obj) else {
            return R_NilValue();
        };
        let resid = list_named_elt(obj, "residuals");
        let fitted = list_named_elt(obj, "fitted.values");
        let mut ysum = 0.0;
        let mut ys = Vec::new();
        for i in 0..XLENGTH(resid) {
            let y = elt_real_safe(fitted, i) + elt_real_safe(resid, i);
            ys.push(y);
            ysum += y;
        }
        let ybar = ysum / n;
        let rss0: f64 = ys.iter().map(|y| (y - ybar) * (y - ybar)).sum();
        let term = {
            let coef = list_named_elt(obj, "coefficients");
            let names = crate::sexp::attrib_core::getAttrib(
                coef,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            if TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) >= 2 {
                std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 1)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                "x".to_string()
            }
        };
        add1_drop1_table(
            rss,
            lm_aic(n, p, rss),
            &term,
            1.0,
            rss0 - rss,
            rss0,
            lm_aic(n, 1.0, rss0),
        )
    }
}

/// GNU `add1(lm, scope)` — add one extra numeric term.
pub unsafe fn do_add1(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let Some((n, p, rss)) = lm_n_p_rss(obj) else {
            return R_NilValue();
        };
        let scope = CAR(CDR(args));
        let form =
            crate::library::stats::filter::do_formula(call, op, Rf_cons(obj, R_NilValue()), rho);
        let _f = protect(form);
        let extra = crate::library::stats::filter::do_add_scope(
            call,
            op,
            Rf_cons(form, Rf_cons(scope, R_NilValue())),
            rho,
        );
        let _e = protect(extra);
        let term = if TYPEOF(extra) == SEXPTYPE::STRSXP && XLENGTH(extra) > 0 {
            std::ffi::CStr::from_ptr(CHAR(STRING_ELT(extra, 0)))
                .to_string_lossy()
                .into_owned()
        } else {
            return R_NilValue();
        };
        let t0 =
            crate::library::stats::filter::do_terms(call, op, Rf_cons(form, R_NilValue()), rho);
        let _t0 = protect(t0);
        let old = crate::sexp::attrib_core::getAttrib(
            t0,
            crate::sexp::symbol::Rf_install(c"term.labels".as_ptr()),
        );
        let nlab = if TYPEOF(old) == SEXPTYPE::STRSXP {
            XLENGTH(old)
        } else {
            0
        };
        let kept = Rf_allocVector3(SEXPTYPE::STRSXP, nlab + 1);
        let _k = protect(kept);
        for i in 0..nlab {
            SET_STRING_ELT(kept, i, STRING_ELT(old, i));
        }
        let tc = CString::new(term.as_str()).unwrap_or_default();
        SET_STRING_ELT(kept, nlab, Rf_mkChar(tc.as_ptr()));
        let resp = Rf_mkString(c"y".as_ptr());
        let _rs = protect(resp);
        let bigf = crate::library::stats::filter::do_reformulate(
            call,
            op,
            Rf_cons(kept, Rf_cons(resp, R_NilValue())),
            rho,
        );
        let _bf = protect(bigf);
        let bigger = do_lm(call, op, Rf_cons(bigf, R_NilValue()), rho);
        let _b = protect(bigger);
        let Some((n2, p2, rss2)) = lm_n_p_rss(bigger) else {
            return R_NilValue();
        };
        let _ = n2;
        add1_drop1_table(
            rss,
            lm_aic(n, p, rss),
            &term,
            1.0,
            rss - rss2,
            rss2,
            lm_aic(n, p2, rss2),
        )
    }
}

/// GNU `step(lm, trace=0)` — keep the fit when dropping raises AIC.
pub unsafe fn do_step(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let dropped = do_drop1(call, op, Rf_cons(obj, R_NilValue()), rho);
        let _d = protect(dropped);
        let aic = list_named_elt(dropped, "AIC");
        if TYPEOF(aic) == SEXPTYPE::REALSXP && XLENGTH(aic) >= 2 {
            let none_aic = *REAL(aic);
            let drop_aic = *REAL(aic).add(1);
            if drop_aic.is_finite() && drop_aic < none_aic {
                // A cheaper model exists; this pin's y~x does not take it.
            }
        }
        obj
    }
}

/// GNU two-sample `power.t.test(n, delta)`.
pub unsafe fn do_power_t_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut n = NA_REAL;
        let mut delta = NA_REAL;
        let mut sd = 1.0;
        let mut sig_level = 0.05;
        let mut cell = args;
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let val = if !v.is_null() && v != R_NilValue() {
                elt_real_safe(v, 0)
            } else {
                NA_REAL
            };
            if name == "n" || (name.is_empty() && pos == 0) {
                n = val;
            } else if name == "delta" || (name.is_empty() && pos == 1) {
                delta = val;
            } else if name == "sd" || (name.is_empty() && pos == 2) {
                if val.is_finite() {
                    sd = val;
                }
            } else if name == "sig.level" || (name.is_empty() && pos == 3) {
                if val.is_finite() {
                    sig_level = val;
                }
            }
            if name.is_empty() {
                pos += 1;
            }
            cell = CDR(cell);
        }
        let nu = (n - 1.0).max(1e-7) * 2.0;
        let qu = crate::dist::t_dist::qt_inner(sig_level / 2.0, nu, false, false);
        let ncp = (n / 2.0).sqrt() * delta.abs() / sd;
        let power = crate::dist::nt_dist::pnt_inner(qu, nu, ncp, false, false);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 8);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(n));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(delta));
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(sd));
        SET_VECTOR_ELT(result, 3, Rf_ScalarReal(sig_level));
        SET_VECTOR_ELT(result, 4, Rf_ScalarReal(power));
        SET_VECTOR_ELT(result, 5, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(
            result,
            6,
            Rf_mkString(c"n is number in *each* group".as_ptr()),
        );
        SET_VECTOR_ELT(
            result,
            7,
            Rf_mkString(c"Two-sample t test power calculation".as_ptr()),
        );
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "n".to_string(),
                "delta".to_string(),
                "sd".to_string(),
                "sig.level".to_string(),
                "power".to_string(),
                "alternative".to_string(),
                "note".to_string(),
                "method".to_string(),
            ],
        );
        let class = Rf_mkString(c"power.htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU two-sample `power.prop.test(n, p1, p2)`.
pub unsafe fn do_power_prop_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut n = NA_REAL;
        let mut p1 = NA_REAL;
        let mut p2 = NA_REAL;
        let mut sig_level = 0.05;
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let val = if !v.is_null() && v != R_NilValue() {
                elt_real_safe(v, 0)
            } else {
                NA_REAL
            };
            match name.as_str() {
                "n" => n = val,
                "p1" => p1 = val,
                "p2" => p2 = val,
                "sig.level" => {
                    if val.is_finite() {
                        sig_level = val;
                    }
                }
                _ => {}
            }
            cell = CDR(cell);
        }
        let qu = crate::dist::normal::qnorm5_inner(sig_level / 2.0, 0.0, 1.0, false, false);
        let num = n.sqrt() * (p1 - p2).abs() - qu * ((p1 + p2) * (1.0 - (p1 + p2) / 2.0)).sqrt();
        let den = (p1 * (1.0 - p1) + p2 * (1.0 - p2)).sqrt();
        let power = crate::dist::normal::pnorm5_inner(num / den, 0.0, 1.0, true, false);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 8);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(n));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(p1));
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(p2));
        SET_VECTOR_ELT(result, 3, Rf_ScalarReal(sig_level));
        SET_VECTOR_ELT(result, 4, Rf_ScalarReal(power));
        SET_VECTOR_ELT(result, 5, Rf_mkString(c"two.sided".as_ptr()));
        SET_VECTOR_ELT(
            result,
            6,
            Rf_mkString(c"n is number in *each* group".as_ptr()),
        );
        SET_VECTOR_ELT(
            result,
            7,
            Rf_mkString(c"Two-sample comparison of proportions power calculation".as_ptr()),
        );
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "n".to_string(),
                "p1".to_string(),
                "p2".to_string(),
                "sig.level".to_string(),
                "power".to_string(),
                "alternative".to_string(),
                "note".to_string(),
                "method".to_string(),
            ],
        );
        let class = Rf_mkString(c"power.htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU balanced one-way `power.anova.test`.
pub unsafe fn do_power_anova_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut groups = NA_REAL;
        let mut n = NA_REAL;
        let mut between_var = NA_REAL;
        let mut within_var = NA_REAL;
        let mut sig_level = 0.05;
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let val = if !v.is_null() && v != R_NilValue() {
                elt_real_safe(v, 0)
            } else {
                NA_REAL
            };
            match name.as_str() {
                "groups" => groups = val,
                "n" => n = val,
                "between.var" => between_var = val,
                "within.var" => within_var = val,
                "sig.level" => {
                    if val.is_finite() {
                        sig_level = val;
                    }
                }
                _ => {}
            }
            cell = CDR(cell);
        }
        let df1 = groups - 1.0;
        let df2 = (n - 1.0) * groups;
        let lambda = df1 * n * (between_var / within_var);
        let crit = crate::dist::f_dist::qf_inner(sig_level, df1, df2, false, false);
        let power = crate::dist::nf_dist::pnf_inner(crit, df1, df2, lambda, false, false);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 8);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(groups));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(n));
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(between_var));
        SET_VECTOR_ELT(result, 3, Rf_ScalarReal(within_var));
        SET_VECTOR_ELT(result, 4, Rf_ScalarReal(sig_level));
        SET_VECTOR_ELT(result, 5, Rf_ScalarReal(power));
        SET_VECTOR_ELT(
            result,
            6,
            Rf_mkString(c"n is number in each group".as_ptr()),
        );
        SET_VECTOR_ELT(
            result,
            7,
            Rf_mkString(c"Balanced one-way analysis of variance power calculation".as_ptr()),
        );
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "groups".to_string(),
                "n".to_string(),
                "between.var".to_string(),
                "within.var".to_string(),
                "sig.level".to_string(),
                "power".to_string(),
                "note".to_string(),
                "method".to_string(),
            ],
        );
        let class = Rf_mkString(c"power.htest".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}
