//! model.frame handling: do_model_frame, expand.model.frame, C_modelframe

use super::*;

/// GNU `model.frame(formula, data)` — columns named in the formula.
pub unsafe fn do_model_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let form = CAR(args);
        let data = CAR(CDR(args));
        if form.is_null() || form == R_NilValue() {
            return R_NilValue();
        }
        let mut names = Vec::new();
        collect_formula_symbols(form, &mut names);
        let mut i_calls = Vec::new();
        collect_i_calls(form, &mut i_calls);
        if names.is_empty() && i_calls.is_empty() {
            return R_NilValue();
        }
        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), _rho, R_NilValue());
        let _env = protect(env);
        if !data.is_null() && data != R_NilValue() && TYPEOF(data) == SEXPTYPE::VECSXP {
            let data_names = crate::sexp::attrib_core::getAttrib(
                data,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            if TYPEOF(data_names) == SEXPTYPE::STRSXP {
                for i in 0..XLENGTH(data) {
                    let nm = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(data_names, i)))
                        .to_string_lossy()
                        .into_owned();
                    if let Ok(c) = std::ffi::CString::new(nm) {
                        crate::sexp::envir::defineVar(
                            crate::sexp::symbol::Rf_install(c.as_ptr()),
                            VECTOR_ELT(data, i),
                            env,
                        );
                    }
                }
            }
        }
        let mut cols: Vec<(String, SEXP)> = Vec::new();
        for name in names {
            let col = if !data.is_null() && data != R_NilValue() {
                named_list_elt(data, &name)
            } else {
                R_NilValue()
            };
            cols.push((name, col));
        }
        for call in i_calls {
            let name = deparse_call(call);
            let val = crate::eval::eval::Rf_eval(call, env);
            cols.push((name, val));
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, cols.len() as i64);
        let _r = protect(result);
        let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, cols.len() as i64);
        let _on = protect(out_names);
        for (i, (name, col)) in cols.iter().enumerate() {
            SET_VECTOR_ELT(result, i as i64, *col);
            let c = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(out_names, i as i64, Rf_mkChar(c.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            out_names,
        );
        let class = Rf_mkString(c"data.frame".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU model.frame removes time-series metadata from evaluated columns while
/// leaving unrelated classes (for example, a leading user class) intact.
/// Work on a duplicate so model.frame never mutates variables in the caller.
unsafe fn model_frame_drop_time_series_attributes(value: SEXP) -> SEXP {
    unsafe {
        if value.is_null()
            || value == R_NilValue()
            || crate::sexp::attrib_core::getAttrib(
                value,
                crate::sexp::attrib_core::R_TspSymbol(),
            ) == R_NilValue()
        {
            return value;
        }

        let duplicate = crate::mainutils::duplicate::Rf_duplicate(value);
        let _duplicate_root = protect(duplicate);
        crate::sexp::attrib_core::setAttrib(
            duplicate,
            crate::sexp::attrib_core::R_TspSymbol(),
            R_NilValue(),
        );

        let class_symbol = crate::sexp::attrib_core::R_ClassSymbol();
        let class = crate::sexp::attrib_core::getAttrib(duplicate, class_symbol);
        if TYPEOF(class) == SEXPTYPE::STRSXP {
            let mut kept = Vec::new();
            for index in 0..XLENGTH(class) {
                let element = STRING_ELT(class, index);
                if element.is_null() || element == crate::sexp::globals::R_NaString() {
                    kept.push(element);
                    continue;
                }
                let name = std::ffi::CStr::from_ptr(CHAR(element)).to_bytes();
                if name != b"ts" && name != b"mts" {
                    kept.push(element);
                }
            }
            if kept.len() as R_xlen_t != XLENGTH(class) {
                if kept.is_empty() {
                    crate::sexp::attrib_core::setAttrib(duplicate, class_symbol, R_NilValue());
                } else {
                    let next = Rf_allocVector3(SEXPTYPE::STRSXP, kept.len() as i64);
                    let _next_root = protect(next);
                    for (index, element) in kept.into_iter().enumerate() {
                        SET_STRING_ELT(next, index as i64, element);
                    }
                    crate::sexp::attrib_core::setAttrib(duplicate, class_symbol, next);
                }
            }
        }
        duplicate
    }
}

/// GNU `expand.model.frame(model, extras, na.expand = FALSE)`.
pub unsafe fn do_expand_model_frame(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let model = CAR(args);
        let extras = CAR(CDR(args));
        let na_expand = expand_na_expand_arg(args);
        let mcall = named_list_elt(model, "call");
        if mcall.is_null() || mcall == R_NilValue() || TYPEOF(mcall) != SEXPTYPE::LANGSXP {
            return R_NilValue();
        }
        let form = CADR(mcall);
        if form.is_null() || form == R_NilValue() {
            return R_NilValue();
        }
        let uargs = Rf_cons(extras, R_NilValue());
        let _u1 = protect(uargs);
        let uargs = Rf_cons(form, uargs);
        let _u2 = protect(uargs);
        let updated = do_update_formula(call, op, uargs, rho);
        let _u = protect(updated);
        let mut names = Vec::new();
        collect_formula_symbols(updated, &mut names);
        collect_extra_names(extras, &mut names);
        if names.is_empty() {
            return R_NilValue();
        }
        let extra_names = {
            let mut extra = Vec::new();
            collect_extra_names(extras, &mut extra);
            extra
        };

        let mut data = pairlist_named(mcall, "data");
        if TYPEOF(data) == SEXPTYPE::SYMSXP || TYPEOF(data) == SEXPTYPE::LANGSXP {
            data = crate::eval::eval::Rf_eval(data, rho);
        }
        let eval_rho = if !data.is_null() && data != R_NilValue() {
            crate::mainutils::essentials::data_environment(data, rho)
        } else {
            rho
        };
        let _er = protect(eval_rho);

        let mut cols: Vec<SEXP> = Vec::with_capacity(names.len());
        let mut nrow: R_xlen_t = 0;
        for name in &names {
            let c = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            let mut col = if !data.is_null() && data != R_NilValue() {
                named_list_elt(data, name)
            } else {
                R_NilValue()
            };
            if col.is_null() || col == R_NilValue() {
                let sym = crate::sexp::symbol::Rf_install(c.as_ptr());
                col = crate::eval::eval::Rf_eval(sym, eval_rho);
            }
            if !col.is_null() && col != R_NilValue() {
                nrow = nrow.max(XLENGTH(col));
            }
            cols.push(col);
        }

        let mut rows = expand_subset_rows(pairlist_named(mcall, "subset"), rho, nrow);
        if !na_expand {
            rows.retain(|&r| {
                extra_names.iter().all(|name| {
                    names
                        .iter()
                        .position(|n| n == name)
                        .and_then(|i| cols.get(i).copied())
                        .is_none_or(|col| !elt_is_na(col, r))
                })
            });
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, names.len() as i64);
        let _r = protect(result);
        let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as i64);
        let _on = protect(out_names);
        for (i, name) in names.iter().enumerate() {
            let col = vector_take_rows(cols[i], &rows);
            SET_VECTOR_ELT(result, i as i64, col);
            let c = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(out_names, i as i64, Rf_mkChar(c.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            out_names,
        );
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, rows.len() as i64);
        let _rn = protect(rn);
        for (i, row) in rows.iter().enumerate() {
            let label = format!("{}", row + 1);
            let c = std::ffi::CString::new(label).unwrap_or_default();
            SET_STRING_ELT(rn, i as i64, Rf_mkChar(c.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_RowNamesSymbol(),
            rn,
        );
        crate::mainutils::essentials::set_data_frame_class(result);
        result
    }
}

fn collect_extra_names(extras: SEXP, out: &mut Vec<String>) {
    unsafe {
        if extras.is_null() || extras == R_NilValue() {
            return;
        }
        if TYPEOF(extras) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(extras) {
                let raw = CHAR(STRING_ELT(extras, i));
                if raw.is_null() {
                    continue;
                }
                let name = std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned();
                if !name.is_empty() && !out.iter().any(|s| s == &name) {
                    out.push(name);
                }
            }
            return;
        }
        collect_formula_symbols(extras, out);
    }
}

unsafe fn expand_na_expand_arg(args: SEXP) -> bool {
    unsafe {
        let v = pairlist_named(args, "na.expand");
        if v.is_null() || v == R_NilValue() {
            false
        } else {
            asBool(v)
        }
    }
}

unsafe fn pairlist_named(list: SEXP, want: &str) -> SEXP {
    unsafe {
        let mut p = if !list.is_null() && TYPEOF(list) == SEXPTYPE::LANGSXP {
            CDR(list)
        } else {
            list
        };
        while !p.is_null() && p != R_NilValue() {
            let tag = TAG(p);
            if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag))).to_string_lossy();
                if name == want {
                    return CAR(p);
                }
            }
            p = CDR(p);
        }
        R_NilValue()
    }
}

unsafe fn expand_subset_rows(subset: SEXP, rho: SEXP, nrow: R_xlen_t) -> Vec<R_xlen_t> {
    unsafe {
        if subset.is_null() || subset == R_NilValue() {
            return (0..nrow).collect();
        }
        let value = if TYPEOF(subset) == SEXPTYPE::SYMSXP || TYPEOF(subset) == SEXPTYPE::LANGSXP {
            crate::eval::eval::Rf_eval(subset, rho)
        } else {
            subset
        };
        if value.is_null() || value == R_NilValue() {
            return (0..nrow).collect();
        }
        let n = XLENGTH(value);
        let mut rows = Vec::with_capacity(n as usize);
        for i in 0..n {
            let idx = match TYPEOF(value) {
                t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                    *INTEGER(value).add(i as usize)
                }
                t if t == SEXPTYPE::REALSXP => *REAL(value).add(i as usize) as i32,
                _ => 0,
            };

            if idx >= 1 && (idx as R_xlen_t) <= nrow {
                rows.push((idx as R_xlen_t) - 1);
            }
        }
        if rows.is_empty() {
            (0..nrow).collect()
        } else {
            rows
        }
    }
}

unsafe fn elt_is_na(x: SEXP, i: R_xlen_t) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() || i < 0 || i >= XLENGTH(x) {
            return true;
        }
        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP => {
                let v = *REAL(x).add(i as usize);
                v.to_bits() == R_NA_BIT_PATTERN || v.is_nan()
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                *INTEGER(x).add(i as usize) == NA_INTEGER
            }
            t if t == SEXPTYPE::STRSXP => {
                let s = STRING_ELT(x, i);
                s.is_null() || s == crate::sexp::globals::R_NaString()
            }
            _ => false,
        }
    }
}

unsafe fn vector_take_rows(x: SEXP, rows: &[R_xlen_t]) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::REALSXP, rows.len() as R_xlen_t);
        }
        let n = rows.len() as R_xlen_t;
        let out = Rf_allocVector3(TYPEOF(x), n);
        if out.is_null() {
            return R_NilValue();
        }
        let src_n = XLENGTH(x);
        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP => {
                for (j, &r) in rows.iter().enumerate() {
                    *REAL(out).add(j) = if r < src_n {
                        *REAL(x).add(r as usize)
                    } else {
                        NA_REAL
                    };
                }
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                for (j, &r) in rows.iter().enumerate() {
                    *INTEGER(out).add(j) = if r < src_n {
                        *INTEGER(x).add(r as usize)
                    } else {
                        NA_INTEGER
                    };
                }
            }
            t if t == SEXPTYPE::STRSXP => {
                for (j, &r) in rows.iter().enumerate() {
                    if r < src_n {
                        SET_STRING_ELT(out, j as R_xlen_t, STRING_ELT(x, r));
                    } else {
                        SET_STRING_ELT(out, j as R_xlen_t, crate::sexp::globals::R_NaString());
                    }
                }
            }
            _ => {}
        }

        out
    }
}

/// GNU `model.response(data)` — first column of a model frame.
pub unsafe fn do_model_response(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let data = CAR(args);
        if data.is_null() || data == R_NilValue() || TYPEOF(data) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        if XLENGTH(data) <= 0 {
            return R_NilValue();
        }
        VECTOR_ELT(data, 0)
    }
}

/// GNU `.External2(C_modelframe, terms, rownames, variables, varnames, dots, ...)`.
pub unsafe fn modelframe(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut args = CDR(args);
        let _terms = CAR(args);
        args = CDR(args);
        let mut row_names = CAR(args);
        args = CDR(args);
        let variables = CAR(args);
        args = CDR(args);
        let varnames = CAR(args);
        args = CDR(args);
        let dots = CAR(args);
        args = CDR(args);
        let dotnames = CAR(args);
        args = CDR(args);
        let subset = CAR(args);
        args = CDR(args);
        let mut na_action = CAR(args);

        if TYPEOF(variables) != SEXPTYPE::VECSXP || TYPEOF(varnames) != SEXPTYPE::STRSXP {
            crate::main::errors::Rf_error(
                b"invalid variables\0".as_ptr() as *const std::os::raw::c_char
            );
        }
        let nvars = XLENGTH(variables);
        if nvars != XLENGTH(varnames) {
            crate::main::errors::Rf_error(
                b"number of variables != number of variable names\0".as_ptr()
                    as *const std::os::raw::c_char,
            );
        }
        let ndots = if dots.is_null() || dots == R_NilValue() || TYPEOF(dots) != SEXPTYPE::VECSXP {
            0
        } else {
            XLENGTH(dots)
        };
        let mut nactual = 0i64;
        for i in 0..ndots {
            let v = VECTOR_ELT(dots, i);
            if !v.is_null() && v != R_NilValue() {
                nactual += 1;
            }
        }
        let data = Rf_allocVector3(SEXPTYPE::VECSXP, nvars + nactual);
        let _d = protect(data);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, nvars + nactual);
        let _n = protect(names);
        let mut column_roots = Vec::new();
        for i in 0..nvars {
            let source = VECTOR_ELT(variables, i);
            let _source_root = protect(source);
            let column = model_frame_drop_time_series_attributes(source);
            column_roots.push(protect(column));
            SET_VECTOR_ELT(data, i, column);
            SET_STRING_ELT(names, i, STRING_ELT(varnames, i));
        }
        let mut j = nvars;
        for i in 0..ndots {
            let v = VECTOR_ELT(dots, i);
            if v.is_null() || v == R_NilValue() {
                continue;
            }
            let _source_root = protect(v);
            let column = model_frame_drop_time_series_attributes(v);
            column_roots.push(protect(column));
            SET_VECTOR_ELT(data, j, column);
            if TYPEOF(dotnames) == SEXPTYPE::STRSXP && i < XLENGTH(dotnames) {
                let raw = CHAR(STRING_ELT(dotnames, i));
                let s = if raw.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned()
                };
                let labeled = format!("({s})");
                let c = std::ffi::CString::new(labeled).unwrap_or_default();
                SET_STRING_ELT(names, j, Rf_mkChar(c.as_ptr()));
            }
            j += 1;
        }
        crate::sexp::attrib_core::setAttrib(data, crate::sexp::attrib_core::R_NamesSymbol(), names);
        let mut nr: R_xlen_t = 0;
        for i in 0..XLENGTH(data) {
            let ans = VECTOR_ELT(data, i);
            let t = TYPEOF(ans);
            let ok = t == SEXPTYPE::LGLSXP
                || t == SEXPTYPE::INTSXP
                || t == SEXPTYPE::REALSXP
                || t == SEXPTYPE::CPLXSXP
                || t == SEXPTYPE::STRSXP
                || t == SEXPTYPE::RAWSXP;
            if !ok {
                let ty = if t == SEXPTYPE::VECSXP {
                    "list"
                } else if t == SEXPTYPE::CLOSXP {
                    "closure"
                } else if t == SEXPTYPE::LANGSXP {
                    "language"
                } else if t == SEXPTYPE::SYMSXP {
                    "symbol"
                } else if t == SEXPTYPE::ENVSXP {
                    "environment"
                } else if t == SEXPTYPE::NILSXP {
                    "NULL"
                } else {
                    "unknown"
                };
                let raw = CHAR(STRING_ELT(names, i));
                let name = if raw.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned()
                };
                let msg =
                    std::ffi::CString::new(format!("invalid type ({ty}) for variable '{name}'"))
                        .unwrap_or_else(|_| {
                            std::ffi::CString::new("invalid type for variable").unwrap()
                        });
                crate::main::errors::Rf_error(msg.as_ptr());
            }
            let rows = model_column_rows(ans);
            if i == 0 {
                nr = rows;
            } else if rows != nr {
                let raw = CHAR(STRING_ELT(names, i));
                let name = if raw.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned()
                };
                let msg = format!("variable lengths differ (found for '{name}')");
                crate::main::errors::errorcall_str(crate::main::errors::findCall(), &msg);
            }
        }
        if XLENGTH(data) == 0 {
            nr = if row_names.is_null() || row_names == R_NilValue() {
                0
            } else {
                XLENGTH(row_names)
            };
        }
        let class = Rf_mkString(c"data.frame".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(data, crate::sexp::attrib_core::R_ClassSymbol(), class);
        if !row_names.is_null() && row_names != R_NilValue() && XLENGTH(row_names) == nr {
            crate::sexp::attrib_core::setAttrib(
                data,
                crate::sexp::attrib_core::R_RowNamesSymbol(),
                row_names,
            );
        } else {
            let compact = Rf_allocVector3(SEXPTYPE::INTSXP, if nr > 0 { 2 } else { 0 });
            let _c = protect(compact);
            if nr > 0 {
                *INTEGER(compact) = NA_INTEGER;
                *INTEGER(compact).add(1) = -(nr as i32);
            }
            crate::sexp::attrib_core::setAttrib(
                data,
                crate::sexp::attrib_core::R_RowNamesSymbol(),
                compact,
            );
        }
        let mut ans = data;
        if !subset.is_null()
            && subset != R_NilValue()
            && subset != crate::sexp::globals::R_MissingArg()
        {
            let index = if TYPEOF(subset) == SEXPTYPE::LANGSXP || TYPEOF(subset) == SEXPTYPE::SYMSXP
            {
                crate::eval::eval::Rf_eval(subset, rho)
            } else {
                subset
            };
            let _idx = protect(index);
            let drop_val = Rf_ScalarLogical(0);
            let _drop_val = protect(drop_val);
            let drop_cell = Rf_cons(drop_val, R_NilValue());
            SETTAG(drop_cell, crate::sexp::symbol::Rf_install(c"drop".as_ptr()));
            let args = Rf_cons(
                data,
                Rf_cons(
                    index,
                    Rf_cons(crate::sexp::globals::R_MissingArg(), drop_cell),
                ),
            );
            let _args = protect(args);
            ans = crate::mainutils::subset::do_subset_dflt(_call, _op, args, rho);
            let _sub = protect(ans);
        }
        if !na_action.is_null() && na_action != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                ans,
                crate::sexp::symbol::Rf_install(c"terms".as_ptr()),
                _terms,
            );
            if TYPEOF(na_action) == SEXPTYPE::STRSXP && XLENGTH(na_action) > 0 {
                let raw = CHAR(STRING_ELT(na_action, 0));
                na_action = crate::sexp::symbol::Rf_install(raw);
            }
            let call_na = Rf_lang2(na_action, ans);
            ans = crate::eval::eval::Rf_eval(call_na, rho);
            let _a = protect(ans);
        }
        ans
    }
}

fn model_column_rows(col: SEXP) -> R_xlen_t {
    unsafe {
        if col.is_null() || col == R_NilValue() {
            return 0;
        }
        let dim = crate::sexp::attrib_core::getAttrib(col, crate::sexp::attrib_core::R_DimSymbol());
        if !dim.is_null() && TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) >= 1 {
            let n = *INTEGER(dim);
            if n > 0 {
                return n as R_xlen_t;
            }
        }
        XLENGTH(col)
    }
}

#[cfg(test)]
mod weighted_revocation_tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use crate::sexp::RSession;

    #[test]
    fn weighted_lm_gc_callback_revocation_refuses_publication() {
        let mut session = RSession::new_for_gc_tests();
        let instance = session.instance_ptr();
        let revoked = Rc::new(Cell::new(false));
        let observed = revoked.clone();
        unsafe {
            (*instance).memory_state.gc_force_gap = 0;
            (*instance).memory_state.gc_force_wait = 0;
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if observed.replace(true) {
                    return;
                }
                crate::sexp::instance::revoke_instance_availability(instance);
            }));
        }

        let (result, _, _) = session.eval_script_with_output_capture(
            "lm(c(1, 2, 4) ~ 1, weights = local({ gc(); c(1, 1, 8) }))",
        );
        assert!(revoked.get(), "the explicit GC in the weight promise must run");
        assert!(result.is_err(), "a revoked owner must not publish a weighted model");
    }
}
