//! stepfun/ecdf/spline/density/cancor/dist related builtins — extracted verbatim from essentials/mathstats.rs.

use super::*;

/// GNU `estVar(SSD)` — `$SSD / $df`.
pub fn do_est_var(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    
        let obj = CAR(args);
        let ssd = list_named_elt(obj, "SSD");
        let df = list_named_elt(obj, "df");
        if ssd == R_NilValue() || df == R_NilValue() {
            return R_NilValue();
        }
        let d = elt_real_safe(df, 0);
        if !d.is_finite() || d == 0.0 {
            return R_NilValue();
        }
        let n = XLENGTH(ssd);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n as usize {
            set_real_checked(result, (i) as R_xlen_t, elt_real_safe(ssd, i as i64) / d);
        }
        let dim = crate::sexp::attrib_core::getAttrib(ssd, crate::sexp::attrib_core::R_DimSymbol());
        if !dim.is_null() && dim != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimSymbol(),
                dim,
            );
        }
        result
    
}

/// GNU `SSD(mlm)` — `crossprod(residuals)` and `$df.residual`.
pub fn do_ssd(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let resid = list_named_elt(obj, "residuals");
        let dfr = list_named_elt(obj, "df.residual");
        if resid == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid) as usize;
        let mut ss = 0.0;
        for i in 0..n {
            let e = elt_real_safe(resid, i as i64);
            ss += e * e;
        }
        let ssd = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 1, 1);
        let _s = protect(ssd);
        set_real_checked(ssd, 0, ss);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, ssd);
        SET_VECTOR_ELT(
            result,
            1,
            if dfr == R_NilValue() {
                Rf_ScalarReal((n as f64) - 2.0)
            } else {
                dfr
            },
        );
        crate::mainutils::essentials::set_string_names(
            result,
            &["SSD".to_string(), "df".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            mk_string_cstr(c"SSD"),
        );
        result
    }
}

/// GNU `simulate(lm, nsim=1)` — `fitted + rnorm(n, sd=sigma)`.
pub fn do_simulate(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        let obj = CAR(args);
        let fitted = list_named_elt(obj, "fitted.values");
        let mut sigma = list_named_elt(obj, "sigma");
        if fitted == R_NilValue() {
            return R_NilValue();
        }
        if sigma == R_NilValue() {
            let resid = list_named_elt(obj, "residuals");
            if resid == R_NilValue() {
                return R_NilValue();
            }
            let n = XLENGTH(resid) as f64;
            let mut ss = 0.0;
            for i in 0..XLENGTH(resid) as usize {
                let e = elt_real_safe(resid, i as i64);
                ss += e * e;
            }
            let df = (n - 2.0).max(1.0);
            sigma = Rf_ScalarReal((ss / df).sqrt());
        }
        let _sg = protect(sigma);
        let mut nsim = 1i32;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                charsxp_text(PRINTNAME(tag)).unwrap_or_default()
            } else {
                String::new()
            };
            if name == "nsim" || name.is_empty() {
                let v = CAR(cell);
                let k = if TYPEOF(v) == SEXPTYPE::INTSXP {
                    integer_elt_checked(v, 0)
                } else {
                    elt_real_safe(v, 0) as i32
                };
                if k > 0 {
                    nsim = k;
                    if name == "nsim" {
                        break;
                    }
                }
            }
            cell = CDR(cell);
        }
        let n = XLENGTH(fitted) as i32;
        let ntot = n * nsim;
        let rargs = Rf_cons(
            Rf_ScalarInteger(ntot),
            Rf_cons(Rf_ScalarReal(0.0), Rf_cons(sigma, R_NilValue())),
        );
        let _ra = protect(rargs);
        let noise = crate::library::stats::random::do_rnorm_r(call, op, rargs, rho);
        let _n = protect(noise);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, nsim as i64);
        let _r = protect(result);
        let mut names = Vec::new();
        for s in 0..nsim as usize {
            let col = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
            let _c = protect(col);
            for i in 0..n as usize {
                set_real_checked(col, (i) as R_xlen_t, elt_real_safe(fitted, i as i64)
                    + elt_real_safe(noise, (s * n as usize + i) as i64));
            }
            SET_VECTOR_ELT(result, s as i64, col);
            names.push(format!("sim_{}", s + 1));
        }
        crate::mainutils::essentials::set_string_names(result, &names);
        result
    }
}

/// GNU `selfStart(model, initial, parameters)` — `pnames` + `initial` + class.
pub fn do_self_start(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let model = CAR(args);
    let initial = CAR(CDR(args));
    let parameters = CAR(CDR(CDR(args)));
    crate::sexp::attrib_core::setAttrib(
        model,
        install_str("initial"),
        if initial.is_null() {
            R_NilValue()
        } else {
            initial
        },
    );
    crate::sexp::attrib_core::setAttrib(
        model,
        install_str("pnames"),
        if parameters.is_null() {
            R_NilValue()
        } else {
            parameters
        },
    );
    crate::sexp::attrib_core::setAttrib(
        model,
        crate::sexp::attrib_core::R_ClassSymbol(),
        mk_string_cstr(c"selfStart"),
    );
    model
}

/// GNU `getInitial(object, data)` — call `attr(object,"initial")`.
pub fn do_get_initial(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let ini = crate::sexp::attrib_core::getAttrib(
            obj,
            install_str("initial"),
        );
        if ini.is_null() || ini == R_NilValue() {
            return R_NilValue();
        }
        let call = crate::sexp::constructors::Rf_lang2(ini, R_NilValue());
        let _c = protect(call);
        crate::eval::eval::Rf_eval(call, rho)
    }
}

fn col_names_of(x: SEXP) -> Vec<String> {
    
        let dn =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimNamesSymbol());
        if !dn.is_null() && dn != R_NilValue() && TYPEOF(dn) == SEXPTYPE::VECSXP && XLENGTH(dn) >= 2
        {
            let cn = VECTOR_ELT(dn, 1);
            if TYPEOF(cn) == SEXPTYPE::STRSXP {
                return (0..XLENGTH(cn))
                    .map(|i| {
                        charsxp_text(STRING_ELT(cn, i)).unwrap_or_default()
                    })
                    .collect();
            }
        }
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if TYPEOF(names) == SEXPTYPE::STRSXP {
            return (0..XLENGTH(names))
                .map(|i| {
                    charsxp_text(STRING_ELT(names, i)).unwrap_or_default()
                })
                .collect();
        }
        Vec::new()
    
}

/// GNU `stat.anova(table, test="Chisq", scale)` — add `Pr(>Chi)`.
pub fn do_stat_anova(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let table = CAR(args);
        let mut scale = 1.0;
        let mut cell = CDR(args);
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                charsxp_text(PRINTNAME(tag)).unwrap_or_default()
            } else {
                String::new()
            };
            if name == "scale" || (name.is_empty() && pos == 1) {
                let v = elt_real_safe(CAR(cell), 0);
                if v.is_finite() && v > 0.0 {
                    scale = v;
                }
            }
            pos += 1;
            cell = CDR(cell);
        }
        let names = col_names_of(table);
        let df_col = names.iter().position(|s| s == "Df");
        let dev_col = names
            .iter()
            .position(|s| s == "Deviance" || s == "Sum of Sq");
        let (Some(df_i), Some(dev_i)) = (df_col, dev_col) else {
            return table;
        };
        let (nr, nc, get) = if TYPEOF(table) == SEXPTYPE::VECSXP {
            let nc = XLENGTH(table) as usize;
            let nr = if nc > 0 {
                XLENGTH(VECTOR_ELT(table, 0)) as usize
            } else {
                0
            };
            let get = Box::new(move |r: usize, c: usize| -> f64 {
                elt_real_safe(VECTOR_ELT(table, c as i64), r as i64)
            }) as Box<dyn Fn(usize, usize) -> f64>;
            (nr, nc, get)
        } else {
            let dim =
                crate::sexp::attrib_core::getAttrib(table, crate::sexp::attrib_core::R_DimSymbol());
            let (nr, nc) = if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) >= 2 {
                (integer_elt_checked(dim, 0) as usize, integer_elt_checked(dim, (1) as R_xlen_t) as usize)
            } else {
                (XLENGTH(table) as usize, 1usize)
            };
            let get = Box::new(move |r: usize, c: usize| -> f64 {
                elt_real_safe(table, (r + c * nr) as i64)
            }) as Box<dyn Fn(usize, usize) -> f64>;
            (nr, nc, get)
        };
        if nr == 0 {
            return table;
        }
        let result = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            nr as i32,
            (nc + 1) as i32,
        );
        let _r = protect(result);
        for c in 0..nc {
            for r in 0..nr {
                set_real_checked(result, (r + c * nr) as R_xlen_t, get(r, c));
            }
        }
        for r in 0..nr {
            let df = get(r, df_i);
            let dev = get(r, dev_i);
            let p = if !df.is_finite() || df == 0.0 || !dev.is_finite() {
                NA_REAL
            } else {
                let vals = dev / scale * df.signum();
                if !vals.is_finite() || vals < 0.0 {
                    NA_REAL
                } else {
                    crate::dist::chisq::pchisq_inner(vals, df.abs(), false, false)
                }
            };
            set_real_checked(result, (r + nc * nr) as R_xlen_t, p);
        }
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, (nc + 1) as i64);
        let _cn = protect(cn);
        for (i, name) in names.iter().enumerate() {
            let c = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(cn, i as i64, mk_char_cstr(&c));
        }
        SET_STRING_ELT(cn, nc as i64, mk_char_cstr(c"Pr(>Chi)"));
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

/// GNU `cophenetic(hclust)` — height of the first common ancestor.
pub fn do_cophenetic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let obj = CAR(args);
    let merge = list_named_elt(obj, "merge");
    let height = list_named_elt(obj, "height");
    if merge == R_NilValue() || height == R_NilValue() {
        return R_NilValue();
    }
    let nmerge = XLENGTH(height) as usize;
    if nmerge == 0 {
        return R_NilValue();
    }
    let n = nmerge + 1;
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); nmerge];
    let mut coph = vec![0.0; n * (n - 1) / 2];
    for s in 0..nmerge {
        let left = integer_elt_checked(merge, (s) as R_xlen_t);
        let right = integer_elt_checked(merge, (s + nmerge) as R_xlen_t);
        let left_m = if left < 0 {
            vec![(-left as usize) - 1]
        } else {
            members[(left as usize) - 1].clone()
        };
        let right_m = if right < 0 {
            vec![(-right as usize) - 1]
        } else {
            members[(right as usize) - 1].clone()
        };
        let h = elt_real_safe(height, s as i64);
        for &i in &left_m {
            for &j in &right_m {
                dist_compact_set(&mut coph, i, j, n, h);
            }
        }
        let mut both = left_m;
        both.extend(right_m);
        members[s] = both;
    }
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, coph.len() as i64);
    let _r = protect(result);
    for (i, v) in coph.iter().enumerate() {
        set_real_checked(result, (i) as R_xlen_t, *v);
    }
    crate::sexp::attrib_core::setAttrib(
        result,
        install_str("Size"),
        Rf_ScalarInteger(n as c_int),
    );
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_ClassSymbol(),
        mk_string_cstr(c"dist"),
    );
    result
}

/// GNU `ecdf(x)` — empirical CDF as a step function.
pub fn do_ecdf(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let xt = TYPEOF(x);
        if xt != SEXPTYPE::INTSXP && xt != SEXPTYPE::REALSXP && xt != SEXPTYPE::LGLSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'x' must be numeric",
            );
        }
        let n0 = XLENGTH(x);
        let mut vals: Vec<f64> = Vec::new();
        for i in 0..n0 {
            let v = if xt == SEXPTYPE::REALSXP {
                real_elt_checked(x, (i as usize) as R_xlen_t)
            } else {
                let iv = integer_elt_checked(x, (i as usize) as R_xlen_t);
                if iv == NA_INTEGER {
                    continue;
                }
                iv as f64
            };
            if v.is_finite() {
                vals.push(v);
            }
        }
        if vals.is_empty() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'x' must have 1 or more non-missing values",
            );
        }
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = vals.len() as f64;
        let mut uniq: Vec<f64> = Vec::new();
        let mut ys: Vec<f64> = Vec::new();
        let mut i = 0;
        while i < vals.len() {
            let v = vals[i];
            let mut j = i + 1;
            while j < vals.len() && vals[j] == v {
                j += 1;
            }
            uniq.push(v);
            ys.push(j as f64 / n);
            i = j;
        }
        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), rho, R_NilValue());
        let _env = protect(env);
        let vx = Rf_allocVector3(SEXPTYPE::REALSXP, uniq.len() as i64);
        let _vx = protect(vx);
        let vy = Rf_allocVector3(SEXPTYPE::REALSXP, ys.len() as i64);
        let _vy = protect(vy);
        for (i, v) in uniq.iter().enumerate() {
            set_real_checked(vx, (i) as R_xlen_t, *v);
            set_real_checked(vy, (i) as R_xlen_t, ys[i]);
        }
        define_var(install_str("vals"), vx, env);
        define_var(install_str("ys"), vy, env);
        define_var(
            install_str("nobs"),
            Rf_ScalarInteger(n as c_int),
            env,
        );
        let vsym = install_str("v");
        let formals = Rf_cons(crate::sexp::globals::R_MissingArg(), R_NilValue());
        SETTAG(formals, vsym);
        let body = crate::sexp::constructors::Rf_lang2(install_str(".ecdf_apply"), vsym);
        let fun = crate::mainutils::dstruct::mkCLOSXP(formals, body, env);
        let _fun = protect(fun);
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        SET_STRING_ELT(class, 0, mk_char_cstr(c"ecdf"));
        SET_STRING_ELT(class, 1, mk_char_cstr(c"stepfun"));
        SET_STRING_ELT(class, 2, mk_char_cstr(c"function"));
        crate::sexp::attrib_core::setAttrib(fun, crate::sexp::attrib_core::R_ClassSymbol(), class);
        fun
    }
}

/// GNU `knots(Fn)` — unique x of an ecdf/stepfun.
pub fn do_knots(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let fun = CAR(args);
    if TYPEOF(fun) != SEXPTYPE::CLOSXP {
        return R_NilValue();
    }
    let env = crate::sexp::accessors::CLOENV(fun);
    for name in [c"x", c"vals"] {
        let v = find_var_in_frame(env, install_cstr(&name));
        if !v.is_null() && v != R_NilValue() && v != crate::sexp::globals::R_UnboundValue() {
            return v;
        }
    }
    R_NilValue()
}

/// GNU `plot.stepfun(x)` — knot/height coordinates (no device).
pub fn do_plot_stepfun(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut fun = CAR(args);
        if TYPEOF(fun) == SEXPTYPE::PROMSXP {
            fun = crate::eval::eval::Rf_eval(fun, rho);
        }
        if fun.is_null() || fun == R_NilValue() || TYPEOF(fun) != SEXPTYPE::CLOSXP {
            return R_NilValue();
        }
        let kn = {
            let env = crate::sexp::accessors::CLOENV(fun);
            let mut found = R_NilValue();
            for name in [c"x", c"vals"] {
                let v = find_var_in_frame(env, install_cstr(&name));
                if !v.is_null() && v != R_NilValue() && v != crate::sexp::globals::R_UnboundValue()
                {
                    found = v;
                    break;
                }
            }
            found
        };
        if kn.is_null() || kn == R_NilValue() || XLENGTH(kn) < 1 {
            return R_NilValue();
        }
        let n = XLENGTH(kn) as usize;
        let mut xval = Vec::with_capacity(n);
        for i in 0..n {
            xval.push(elt_real_safe(kn, i as i64));
        }
        let rx0 = xval[0];
        let rx1 = xval[n - 1];
        let span = rx1 - rx0;
        let med = if n >= 2 {
            let mut diffs: Vec<f64> = xval.windows(2).map(|w| w[1] - w[0]).collect();
            diffs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let m = diffs.len();
            if m % 2 == 1 {
                diffs[m / 2]
            } else {
                (diffs[m / 2 - 1] + diffs[m / 2]) / 2.0
            }
        } else {
            rx0.abs() / 16.0
        };
        let dr = (0.08 * span).max(med);
        let mut ti = Vec::with_capacity(n + 2);
        ti.push(rx0 - 2.0 * dr);
        ti.extend_from_slice(&xval);
        ti.push(rx1 + 2.0 * dr);
        let nt = ti.len();
        let tvec = Rf_allocVector3(SEXPTYPE::REALSXP, nt as i64);
        let _t = protect(tvec);
        for (i, v) in ti.iter().enumerate() {
            set_real_checked(tvec, (i) as R_xlen_t, *v);
        }
        let mids = Rf_allocVector3(SEXPTYPE::REALSXP, (nt - 1) as i64);
        let _m = protect(mids);
        for i in 0..nt - 1 {
            set_real_checked(mids, (i) as R_xlen_t, 0.5 * (ti[i] + ti[i + 1]));
        }
        let yv = stepfun_heights(fun, mids);
        let _y = protect(yv);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, tvec);
        SET_VECTOR_ELT(result, 1, yv);
        crate::mainutils::essentials::set_string_names(result, &["t".to_string(), "y".to_string()]);
        result
    }
}
fn stepfun_heights(fun: SEXP, mids: SEXP) -> SEXP {
    let env = crate::sexp::accessors::CLOENV(fun);
    let x = find_var_in_frame(env, install_str("x"));
    let y = find_var_in_frame(env, install_str("y"));
    let nv = XLENGTH(mids) as usize;
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, nv as i64);
    if x.is_null() || y.is_null() || x == crate::sexp::globals::R_UnboundValue() {
        return result;
    }
    let n = XLENGTH(x) as usize;
    for j in 0..nv {
        let t = elt_real_safe(mids, j as i64);
        let mut i = 0usize;
        while i < n && elt_real_safe(x, i as i64) <= t {
            i += 1;
        }
        set_real_checked(result, (j) as R_xlen_t, elt_real_safe(y, i as i64));
    }
    result
}

/// GNU `plot.ecdf(x)` — `plot.stepfun` then `abline`; returns invisible NULL.
pub fn do_plot_ecdf(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        let _ = do_plot_stepfun(call, op, args, rho);
        R_NilValue()
    }
}

/// GNU `ppplot(x, y, plot.it=FALSE)` — P-P stepfun from two ecdfs.
pub fn do_ppplot(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    
        let x = CAR(args);
        let y = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
            return R_NilValue();
        }
        let nx = XLENGTH(x) as usize;
        let ny = XLENGTH(y) as usize;
        let mut xv = Vec::with_capacity(nx);
        let mut yv = Vec::with_capacity(ny);
        for i in 0..nx {
            let v = elt_real_safe(x, i as i64);
            if v.is_finite() {
                xv.push(v);
            }
        }
        for i in 0..ny {
            let v = elt_real_safe(y, i as i64);
            if v.is_finite() {
                yv.push(v);
            }
        }
        if xv.is_empty() || yv.is_empty() {
            return R_NilValue();
        }
        let mut sy = xv.clone();
        sy.extend_from_slice(&yv);
        sy.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        sy.dedup();
        let nxf = xv.len() as f64;
        let nyf = yv.len() as f64;
        let px = Rf_allocVector3(SEXPTYPE::REALSXP, sy.len() as i64);
        let _px = protect(px);
        let py = Rf_allocVector3(SEXPTYPE::REALSXP, (sy.len() + 1) as i64);
        let _py = protect(py);
        set_real_checked(py, 0, 0.0);
        for (i, t) in sy.iter().enumerate() {
            let cx = xv.iter().filter(|v| *v <= t).count() as f64 / nxf;
            let cy = yv.iter().filter(|v| *v <= t).count() as f64 / nyf;
            set_real_checked(px, (i) as R_xlen_t, cx);
            set_real_checked(py, (i + 1) as R_xlen_t, cy);
        }
        let step_args = Rf_cons(px, Rf_cons(py, R_NilValue()));
        let _sa = protect(step_args);
        do_stepfun(call, op, step_args, rho)
    
}

/// GNU `rfree1way(n, delta=0)` — two-group Control/B; logit shift on treated.
pub fn do_rfree1way(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let n = elt_real_safe(CAR(args), 0).floor() as i32;
    if n < 1 {
        return R_NilValue();
    }
    let mut delta = 0.0;
    let mut cell = CDR(args);
    let mut pos = 0;
    while !cell.is_null() && cell != R_NilValue() {
        let tag = TAG(cell);
        let name = if !tag.is_null() && tag != R_NilValue() {
            charsxp_cstring(PRINTNAME(tag))
                .to_string_lossy()
                .into_owned()
        } else {
            String::new()
        };
        let val = CAR(cell);
        if name == "delta" || (name.is_empty() && pos == 4) {
            delta = elt_real_safe(val, 0);
        }
        if name.is_empty() {
            pos += 1;
        }
        cell = CDR(cell);
    }
    let ntot = (2 * n) as i64;
    let n_s = Rf_ScalarInteger(ntot as c_int);
    let _ns = protect(n_s);
    let a_s = Rf_ScalarReal(0.0);
    let _as = protect(a_s);
    let b_s = Rf_ScalarReal(1.0);
    let _bs = protect(b_s);
    let y = crate::library::stats::random::do_runif(n_s, a_s, b_s);
    let _y = protect(y);
    if delta.abs() > 0.0 {
        for i in n as usize..ntot as usize {
            let u = real_elt_checked(y, (i) as R_xlen_t);
            let q = crate::dist::logistic::qlogis_inner(u, 0.0, 1.0, true, false);
            set_real_checked(y, (i) as R_xlen_t, crate::dist::logistic::plogis_inner(q + delta, 0.0, 1.0, true, false));
        }
    }
    let groups = Rf_allocVector3(SEXPTYPE::INTSXP, ntot);
    let _g = protect(groups);
    for i in 0..n as usize {
        set_integer_checked(groups, (i) as R_xlen_t, 1);
        set_integer_checked(groups, (i + n as usize) as R_xlen_t, 2);
    }
    let lev = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
    let _lv = protect(lev);
    SET_STRING_ELT(lev, 0, mk_char_cstr(c"Control"));
    SET_STRING_ELT(lev, 1, mk_char_cstr(c"B"));
    crate::sexp::attrib_core::setAttrib(
        groups,
        crate::sexp::attrib_core::R_LevelsSymbol(),
        lev,
    );
    let fclass = mk_string_cstr(c"factor");
    let _fc = protect(fclass);
    crate::sexp::attrib_core::setAttrib(
        groups,
        crate::sexp::attrib_core::R_ClassSymbol(),
        fclass,
    );
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    let _r = protect(result);
    SET_VECTOR_ELT(result, 0, groups);
    SET_VECTOR_ELT(result, 1, y);
    crate::mainutils::essentials::set_string_names(
        result,
        &["groups".to_string(), "y".to_string()],
    );
    let rn = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    let _rn = protect(rn);
    set_integer_checked(rn, 0, NA_INTEGER);
    set_integer_checked(rn, (1) as R_xlen_t, -(ntot as i32));
    crate::sexp::attrib_core::setAttrib(
        result,
        install_str("row.names"),
        rn,
    );
    let class = mk_string_cstr(c"data.frame");
    let _cl = protect(class);
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_ClassSymbol(),
        class,
    );
    result
}

/// GNU `stepfun(x, y)` — cadlag step function, `length(y)=length(x)+1`.
pub fn do_stepfun(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let n = XLENGTH(x);
        if n < 1 || XLENGTH(y) != n + 1 {
            return R_NilValue();
        }
        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), rho, R_NilValue());
        let _env = protect(env);
        define_var(install_str("x"), x, env);
        define_var(install_str("y"), y, env);
        let vsym = install_str("v");
        let formals = Rf_cons(crate::sexp::globals::R_MissingArg(), R_NilValue());
        SETTAG(formals, vsym);
        let body =
            crate::sexp::constructors::Rf_lang2(install_str(".stepfun_apply"), vsym);
        let fun = crate::mainutils::dstruct::mkCLOSXP(formals, body, env);
        let _fun = protect(fun);
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        SET_STRING_ELT(class, 0, mk_char_cstr(c"stepfun"));
        SET_STRING_ELT(class, 1, mk_char_cstr(c"function"));
        crate::sexp::attrib_core::setAttrib(fun, crate::sexp::attrib_core::R_ClassSymbol(), class);
        fun
    }
}

/// Evaluate `stepfun` at `v`.
pub fn do_stepfun_apply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let v = CAR(args);
    let env = crate::sexp::accessors::ENCLOS(rho);
    let x = find_var_in_frame(env, install_str("x"));
    let y = find_var_in_frame(env, install_str("y"));
    if x.is_null() || y.is_null() || x == crate::sexp::globals::R_UnboundValue() {
        return R_NilValue();
    }
    let n = XLENGTH(x) as usize;
    let nv = XLENGTH(v) as usize;
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, nv as i64);
    let _r = protect(result);
    for j in 0..nv {
        let t = elt_real_safe(v, j as i64);
        let mut i = 0usize;
        while i < n && elt_real_safe(x, i as i64) <= t {
            i += 1;
        }
        set_real_checked(result, (j) as R_xlen_t, elt_real_safe(y, i as i64));
    }
    result
}

/// GNU `splinefunH(x, y, m)` — cubic Hermite interpolant.
pub fn do_splinefun_h(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let m = CAR(CDR(CDR(args)));
        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), rho, R_NilValue());
        let _env = protect(env);
        define_var(install_str("x"), x, env);
        define_var(install_str("y"), y, env);
        define_var(install_str("m"), m, env);
        let vsym = install_str("v");
        let formals = Rf_cons(crate::sexp::globals::R_MissingArg(), R_NilValue());
        SETTAG(formals, vsym);
        let body =
            crate::sexp::constructors::Rf_lang2(install_str(".splinefunH_apply"), vsym);
        crate::mainutils::dstruct::mkCLOSXP(formals, body, env)
    }
}

/// Evaluate cubic Hermite at `v`.
pub fn do_splinefun_h_apply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let v = CAR(args);
    let env = crate::sexp::accessors::ENCLOS(rho);
    let x = find_var_in_frame(env, install_str("x"));
    let y = find_var_in_frame(env, install_str("y"));
    let m = find_var_in_frame(env, install_str("m"));
    let n = XLENGTH(x) as usize;
    if n < 2 {
        return R_NilValue();
    }
    let nv = XLENGTH(v) as usize;
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, nv as i64);
    let _r = protect(result);
    for j in 0..nv {
        let t0 = elt_real_safe(v, j as i64);
        let mut i = 0usize;
        while i + 1 < n && elt_real_safe(x, (i + 1) as i64) <= t0 {
            i += 1;
        }
        if i + 1 >= n {
            i = n - 2;
        }
        let x0 = elt_real_safe(x, i as i64);
        let x1 = elt_real_safe(x, (i + 1) as i64);
        let h = x1 - x0;
        let t = if h != 0.0 { (t0 - x0) / h } else { 0.0 };
        let t2 = t * t;
        let t3 = t2 * t;
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        let y0 = elt_real_safe(y, i as i64);
        let y1 = elt_real_safe(y, (i + 1) as i64);
        let m0 = elt_real_safe(m, i as i64);
        let m1 = elt_real_safe(m, (i + 1) as i64);
        set_real_checked(result, (j) as R_xlen_t, h00 * y0 + h10 * h * m0 + h01 * y1 + h11 * h * m1);
    }
    result
}

/// Evaluate an ecdf closure: last y with vals <= v, else 0 / 1 at ends.
pub fn do_ecdf_apply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let v = CAR(args);
    let parent = crate::sexp::accessors::ENCLOS(rho);
    let vals = find_var(install_str("vals"), parent);
    let ys = find_var(install_str("ys"), parent);
    if vals.is_null()
        || ys.is_null()
        || TYPEOF(vals) != SEXPTYPE::REALSXP
        || TYPEOF(ys) != SEXPTYPE::REALSXP
    {
        return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
    }
    let nv = XLENGTH(vals) as usize;
    let nq = XLENGTH(v);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, nq);
    for i in 0..nq {
        let q = if TYPEOF(v) == SEXPTYPE::REALSXP {
            real_elt_checked(v, (i as usize) as R_xlen_t)
        } else if TYPEOF(v) == SEXPTYPE::INTSXP {
            integer_elt_checked(v, (i as usize) as R_xlen_t) as f64
        } else {
            NA_REAL
        };
        let mut y = 0.0;
        for j in 0..nv {
            if real_elt_checked(vals, (j) as R_xlen_t) <= q {
                y = real_elt_checked(ys, (j) as R_xlen_t);
            } else {
                break;
            }
        }
        set_real_checked(result, (i as usize) as R_xlen_t, y);
    }
    result
}
