//! Misc model helpers: .getXlevels, .MFclass, vcov.aliased, all.vars, model.extract

use super::*;

/// GNU `.nknots.smspl(n)` — default smooth.spline knot count.
pub unsafe fn do_nknots_smspl(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if TYPEOF(n_s) == SEXPTYPE::INTSXP {
            *INTEGER(n_s)
        } else {
            *REAL(n_s) as i32
        };
        if n < 50 {
            return Rf_ScalarInteger(n);
        }
        let nf = n as f64;
        let a1 = 50f64.log2();
        let a2 = 100f64.log2();
        let a3 = 140f64.log2();
        let a4 = 200f64.log2();
        let knots = if n < 200 {
            2f64.powf(a1 + (a2 - a1) * (nf - 50.0) / 150.0)
        } else if n < 800 {
            2f64.powf(a2 + (a3 - a2) * (nf - 200.0) / 600.0)
        } else if n < 3200 {
            2f64.powf(a3 + (a4 - a3) * (nf - 800.0) / 2400.0)
        } else {
            200.0 + (nf - 3200.0).powf(0.2)
        };
        Rf_ScalarInteger(knots.trunc() as i32)
    }
}

/// GNU `.getXlevels(Terms, m)` — factor levels from the model frame.
pub unsafe fn do_get_xlevels(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = CAR(CDR(args));
        if m.is_null() || TYPEOF(m) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names =
            crate::sexp::attrib_core::getAttrib(m, crate::sexp::attrib_core::R_NamesSymbol());
        let mut out_cols: Vec<SEXP> = Vec::new();
        let mut out_names: Vec<String> = Vec::new();
        for j in 0..XLENGTH(m) {
            let col = VECTOR_ELT(m, j);
            let class =
                crate::sexp::attrib_core::getAttrib(col, crate::sexp::attrib_core::R_ClassSymbol());
            let mut is_fac = false;
            if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
                for i in 0..XLENGTH(class) {
                    if std::ffi::CStr::from_ptr(CHAR(STRING_ELT(class, i))).to_bytes() == b"factor"
                    {
                        is_fac = true;
                        break;
                    }
                }
            }
            if !is_fac {
                continue;
            }
            let lev = crate::sexp::attrib_core::getAttrib(
                col,
                crate::sexp::symbol::Rf_install(c"levels".as_ptr()),
            );
            if lev.is_null() || TYPEOF(lev) != SEXPTYPE::STRSXP {
                continue;
            }
            out_cols.push(lev);
            if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP && j < XLENGTH(names) {
                out_names.push(
                    std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, j)))
                        .to_string_lossy()
                        .into_owned(),
                );
            } else {
                out_names.push(format!("V{}", j + 1));
            }
        }
        if out_cols.is_empty() {
            return R_NilValue();
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, out_cols.len() as i64);
        let _r = protect(result);
        for (i, col) in out_cols.iter().enumerate() {
            SET_VECTOR_ELT(result, i as i64, *col);
        }
        crate::mainutils::essentials::set_string_names(result, &out_names);
        result
    }
}

/// GNU `.MFclass(x)` — model-frame column class label.
pub unsafe fn do_mfclass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        let mut is_factor = false;
        let mut is_ordered = false;
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(class) {
                let s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(class, i)));
                if s.to_bytes() == b"ordered" {
                    is_ordered = true;
                }
                if s.to_bytes() == b"factor" {
                    is_factor = true;
                }
            }
        }
        let label = if TYPEOF(x) == SEXPTYPE::LGLSXP {
            "logical"
        } else if is_ordered {
            "ordered"
        } else if is_factor {
            "factor"
        } else if TYPEOF(x) == SEXPTYPE::STRSXP {
            "character"
        } else {
            let dim =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
            let is_mat = !dim.is_null()
                && dim != R_NilValue()
                && TYPEOF(dim) == SEXPTYPE::INTSXP
                && XLENGTH(dim) >= 2;
            let numeric = TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(x) == SEXPTYPE::INTSXP;
            if is_mat && numeric {
                let ncol = *INTEGER(dim).add(1);
                let text = format!("nmatrix.{ncol}");
                return Rf_mkString(std::ffi::CString::new(text).unwrap().as_ptr());
            } else if numeric {
                "numeric"
            } else {
                "other"
            }
        };
        Rf_mkString(std::ffi::CString::new(label).unwrap().as_ptr())
    }
}

/// GNU `.checkMFClasses(cl, m)` — NULL when named types match.
pub unsafe fn do_check_mf_classes(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let cl = CAR(args);
        let m = CAR(CDR(args));
        if cl.is_null()
            || TYPEOF(cl) != SEXPTYPE::STRSXP
            || XLENGTH(cl) == 0
            || m.is_null()
            || TYPEOF(m) != SEXPTYPE::VECSXP
        {
            return R_NilValue();
        }
        let cl_names =
            crate::sexp::attrib_core::getAttrib(cl, crate::sexp::attrib_core::R_NamesSymbol());
        if cl_names.is_null() || TYPEOF(cl_names) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        let m_names =
            crate::sexp::attrib_core::getAttrib(m, crate::sexp::attrib_core::R_NamesSymbol());
        if m_names.is_null() || TYPEOF(m_names) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        for i in 0..XLENGTH(cl) {
            let want = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(cl, i)))
                .to_string_lossy()
                .into_owned();
            let cname = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(cl_names, i)))
                .to_string_lossy()
                .into_owned();
            for j in 0..XLENGTH(m_names) {
                let mn = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(m_names, j)))
                    .to_string_lossy()
                    .into_owned();
                if mn == cname {
                    let col = VECTOR_ELT(m, j);
                    let got = if TYPEOF(col) == SEXPTYPE::REALSXP || TYPEOF(col) == SEXPTYPE::INTSXP
                    {
                        "numeric"
                    } else {
                        "other"
                    };
                    if got != want {
                        return R_NilValue();
                    }
                }
            }
        }
        R_NilValue()
    }
}

/// GNU `.vcov.aliased(aliased, vc)` — pad NA rows/cols for aliased coefs.
pub unsafe fn do_vcov_aliased(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let aliased = CAR(args);
        let vc = CAR(CDR(args));
        let p = XLENGTH(aliased) as usize;
        let mut keep: Vec<usize> = Vec::new();
        for i in 0..p {
            let a = if TYPEOF(aliased) == SEXPTYPE::LGLSXP {
                *LOGICAL(aliased).add(i) != 0
            } else if TYPEOF(aliased) == SEXPTYPE::INTSXP {
                *INTEGER(aliased).add(i) != 0
            } else {
                *REAL(aliased).add(i) != 0.0
            };
            if !a {
                keep.push(i);
            }
        }
        if keep.len() == p {
            return vc;
        }
        let result =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), p as i32, p as i32);
        let _r = protect(result);
        for i in 0..p * p {
            *REAL(result).add(i) = NA_REAL;
        }
        let k = keep.len();
        for (ii, &i) in keep.iter().enumerate() {
            for (jj, &j) in keep.iter().enumerate() {
                let src = if TYPEOF(vc) == SEXPTYPE::REALSXP {
                    *REAL(vc).add(ii + jj * k)
                } else {
                    *INTEGER(vc).add(ii + jj * k) as f64
                };
                *REAL(result).add(i + j * p) = src;
            }
        }
        result
    }
}

/// GNU `makepredictcall(var, call)` — return `call` unchanged.
pub unsafe fn do_makepredictcall(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let var = CAR(args);
        let pred = CAR(CDR(args));
        if !var.is_null()
            && var != R_NilValue()
            && crate::mainutils::objects::inherits2(var, c"poly".as_ptr()) != 0
        {
            let sym = crate::sexp::symbol::Rf_install(c"makepredictcall.poly".as_ptr());
            let fun = crate::sexp::envir::findFun(sym, rho);
            if !fun.is_null() && fun != crate::sexp::globals::R_UnboundValue() {
                let lang = crate::sexp::constructors::Rf_lang3(fun, var, pred);
                return crate::eval::eval::Rf_eval(lang, rho);
            }
        }
        let _ = (call, op);
        pred
    }
}

/// GNU `as.formula(object)`.
pub unsafe fn do_as_formula(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_formula(call, op, args, rho) }
}

/// GNU `asOneSidedFormula(object)` — `~expr`.
pub unsafe fn do_as_one_sided_formula(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if inherits_formula(x) {
            return x;
        }
        if TYPEOF(x) == SEXPTYPE::LANGSXP {
            let op = CAR(x);
            let name = if !op.is_null() && TYPEOF(op) == SEXPTYPE::SYMSXP {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(op)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "~" {
                return mark_formula(x);
            }
        }
        let rhs = if TYPEOF(x) == SEXPTYPE::STRSXP && XLENGTH(x) > 0 {
            let raw = CHAR(STRING_ELT(x, 0));
            let lab = std::ffi::CStr::from_ptr(raw).to_string_lossy();
            let c = std::ffi::CString::new(lab.as_ref()).unwrap_or_default();
            crate::sexp::symbol::Rf_install(c.as_ptr())
        } else if TYPEOF(x) == SEXPTYPE::SYMSXP {
            x
        } else {
            return R_NilValue();
        };
        let tilde = crate::sexp::symbol::Rf_install(c"~".as_ptr());
        mark_formula(Rf_lang2(tilde, rhs))
    }
}

/// GNU `get_all_vars(formula, data)` — formula columns from `data`.
pub unsafe fn do_get_all_vars(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_model_frame(call, op, args, rho) }
}

fn pack_allnames_args(expr: SEXP, functions: bool) -> SEXP {
    unsafe {
        let unique = Rf_ScalarLogical(1);
        let maxn = Rf_ScalarInteger(-1);
        let funs = Rf_ScalarLogical(if functions { 1 } else { 0 });
        let a1 = Rf_cons(unique, R_NilValue());
        let _a1 = protect(a1);
        let a2 = Rf_cons(maxn, a1);
        let _a2 = protect(a2);
        let a3 = Rf_cons(funs, a2);
        let _a3 = protect(a3);
        let a4 = Rf_cons(expr, a3);
        let _a4 = protect(a4);
        a4
    }
}

/// GNU `all.vars(expr)` — `.Internal(all.names(..., functions=FALSE))`.
pub unsafe fn do_all_vars(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::list::do_allnames(
            call,
            std::ptr::null_mut(),
            pack_allnames_args(CAR(args), false),
            rho,
        )
    }
}

/// GNU `all.names(expr)` — `.Internal(all.names(..., functions=TRUE))`.
pub unsafe fn do_all_names(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::list::do_allnames(
            call,
            std::ptr::null_mut(),
            pack_allnames_args(CAR(args), true),
            rho,
        )
    }
}

/// GNU `model.extract(frame, component)` — `response` or `(component)`.
pub unsafe fn do_model_extract(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let frame = crate::eval::eval::Rf_eval(CAR(args), rho);
        let spec = CAR(CDR(args));
        let name = if TYPEOF(spec) == SEXPTYPE::SYMSXP {
            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(spec)))
                .to_string_lossy()
                .into_owned()
        } else if TYPEOF(spec) == SEXPTYPE::STRSXP && XLENGTH(spec) > 0 {
            std::ffi::CStr::from_ptr(CHAR(STRING_ELT(spec, 0)))
                .to_string_lossy()
                .into_owned()
        } else {
            String::new()
        };
        if name == "response" {
            if TYPEOF(frame) == SEXPTYPE::VECSXP && XLENGTH(frame) > 0 {
                return VECTOR_ELT(frame, 0);
            }
            return R_NilValue();
        }
        if name == "offset" {
            return named_list_elt(frame, "offset");
        }
        let key = format!("({name})");
        named_list_elt(frame, &key)
    }
}

/// GNU `offset(object)` is identity.
pub unsafe fn do_offset(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { CAR(args) }
}

/// GNU default `getCall(x)` via $call.
pub unsafe fn do_getCall(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { named_list_elt(CAR(args), "call") }
}
