//! model.matrix construction and contrast machinery (C_modelmatrix, contr.*)

use super::*;

fn pure_interaction_width(rhs: SEXP, data: SEXP) -> Option<f64> {
    unsafe fn walk(expr: SEXP, vars: &mut Vec<String>) {
        unsafe {
            if expr.is_null() || expr == R_NilValue() {
                return;
            }
            if TYPEOF(expr) == SEXPTYPE::SYMSXP {
                vars.push(
                    std::ffi::CStr::from_ptr(CHAR(PRINTNAME(expr)))
                        .to_string_lossy()
                        .into_owned(),
                );
                return;
            }
            if TYPEOF(expr) != SEXPTYPE::LANGSXP {
                return;
            }
            let op = CAR(expr);
            let name = if TYPEOF(op) == SEXPTYPE::SYMSXP {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(op)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name != ":" && name != "+" && name != "-" {
                return;
            }
            let mut cell = CDR(expr);
            while !cell.is_null() && cell != R_NilValue() {
                walk(CAR(cell), vars);
                cell = CDR(cell);
            }
        }
    }
    unsafe {
        let mut vars = Vec::new();
        walk(rhs, &mut vars);
        if vars.len() < 2 || (TYPEOF(data) != SEXPTYPE::VECSXP) {
            return None;
        }
        let names =
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_NamesSymbol());
        let mut width = 1.0f64;
        for var in &vars {
            let mut col = R_NilValue();
            if TYPEOF(names) == SEXPTYPE::STRSXP {
                for i in 0..XLENGTH(names) {
                    let nm = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i))).to_string_lossy();
                    if nm == var.as_str() {
                        col = VECTOR_ELT(data, i);
                        break;
                    }
                }
            }
            let levels = crate::sexp::attrib_core::getAttrib(
                col,
                crate::sexp::symbol::Rf_install(c"levels".as_ptr()),
            );
            let nlev = if TYPEOF(levels) == SEXPTYPE::STRSXP && XLENGTH(levels) > 0 {
                XLENGTH(levels) as f64
            } else if TYPEOF(col) == SEXPTYPE::STRSXP && col != R_NilValue() {
                // GNU modelmatrix.c factors character columns first; the
                // guard must count distinct values as the level count.
                let mut distinct: Vec<SEXP> = Vec::new();
                for i in 0..XLENGTH(col) {
                    let elt = STRING_ELT(col, i);
                    if elt == crate::sexp::globals::R_NaString() {
                        continue;
                    }
                    if !distinct.iter().any(|&d| {
                        crate::sexp::accessors::CHAR(d) == crate::sexp::accessors::CHAR(elt)
                    }) {
                        distinct.push(elt);
                    }
                }
                distinct.len() as f64
            } else {
                return None;
            };
            width *= nlev;
        }
        Some(width)
    }
}

/// GNU `model.matrix(~x)` — intercept plus one numeric column.
pub unsafe fn do_model_matrix(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let form = CAR(args);
        if form.is_null() || form == R_NilValue() || TYPEOF(form) != SEXPTYPE::LANGSXP {
            return R_NilValue();
        }
        let rhs_cell = CDR(CDR(form));
        let rhs = if rhs_cell.is_null() || rhs_cell == R_NilValue() {
            CADR(form)
        } else {
            CAR(rhs_cell)
        };
        if rhs.is_null() || rhs == R_NilValue() {
            return R_NilValue();
        }
        let data = CAR(CDR(args));
        if let Some(width) = pure_interaction_width(rhs, data) {
            if width > i32::MAX as f64 {
                let shown = format!("{width:.0e}").replace('e', "e+");
                let msg = std::ffi::CString::new(format!("term 1 would require {shown} columns"))
                    .unwrap_or_default();
                crate::main::errors::Rf_error(msg.as_ptr());
            }
        }
        let xname = if TYPEOF(rhs) == SEXPTYPE::SYMSXP {
            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(rhs)))
                .to_string_lossy()
                .into_owned()
        } else {
            "x".to_string()
        };
        let x = crate::eval::eval::Rf_eval(rhs, rho);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        if n <= 0 {
            return R_NilValue();
        }
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, 2);
        let _m = protect(mat);
        for i in 0..n as usize {
            *REAL(mat).add(i) = 1.0;
            let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                let iv = *INTEGER(x).add(i);
                if iv == NA_INTEGER {
                    f64::NAN
                } else {
                    iv as f64
                }
            } else {
                f64::NAN
            };
            *REAL(mat).add(i + n as usize) = v;
        }
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _rn = protect(rn);
        for i in 0..n {
            let lab = format!("{}\0", i + 1);
            SET_STRING_ELT(rn, i, Rf_mkChar(lab.as_ptr() as *const _));
        }
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _cn = protect(cn);
        SET_STRING_ELT(cn, 0, Rf_mkChar(c"(Intercept)".as_ptr()));
        let xn = std::ffi::CString::new(xname).unwrap_or_default();
        SET_STRING_ELT(cn, 1, Rf_mkChar(xn.as_ptr()));
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(mat, crate::sexp::attrib_core::R_DimNamesSymbol(), dn);
        let assign = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        let _as = protect(assign);
        *INTEGER(assign) = 0;
        *INTEGER(assign).add(1) = 1;
        crate::sexp::attrib_core::setAttrib(
            mat,
            crate::sexp::symbol::Rf_install(c"assign".as_ptr()),
            assign,
        );
        mat
    }
}

/// GNU `model.matrix.default(y ~ x, data)` — intercept plus `x`.
pub unsafe fn do_model_matrix_default(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let data = CAR(CDR(args));
        if data.is_null() || data == R_NilValue() || TYPEOF(data) != SEXPTYPE::VECSXP {
            return do_model_matrix(call, op, args, rho);
        }
        let form = CAR(args);
        let rhs_cell = if TYPEOF(form) == SEXPTYPE::LANGSXP {
            CDR(CDR(form))
        } else {
            std::ptr::null_mut()
        };
        let rhs = if rhs_cell.is_null() || rhs_cell == R_NilValue() {
            if TYPEOF(form) == SEXPTYPE::LANGSXP {
                CADR(form)
            } else {
                std::ptr::null_mut()
            }
        } else {
            CAR(rhs_cell)
        };
        let xname = if !rhs.is_null() && TYPEOF(rhs) == SEXPTYPE::SYMSXP {
            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(rhs)))
                .to_string_lossy()
                .into_owned()
        } else {
            "x".to_string()
        };
        let names =
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_NamesSymbol());
        let mut col = R_NilValue();
        if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(names) {
                let nm = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i))).to_string_lossy();
                if nm == xname {
                    col = VECTOR_ELT(data, i);
                    break;
                }
            }
        }
        if col.is_null() || col == R_NilValue() {
            return do_model_matrix(call, op, args, rho);
        }
        let n = XLENGTH(col);
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, 2);
        let _m = protect(mat);
        for i in 0..n as usize {
            *REAL(mat).add(i) = 1.0;
            *REAL(mat).add(i + n as usize) = if TYPEOF(col) == SEXPTYPE::REALSXP {
                *REAL(col).add(i)
            } else {
                *INTEGER(col).add(i) as f64
            };
        }
        mat
    }
}

/// GNU `model.matrix.lm(object)` — intercept plus `1:n` when `$x` is missing.
pub unsafe fn do_model_matrix_lm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let x = named_list_elt(obj, "x");
        if !x.is_null() && x != R_NilValue() {
            return x;
        }
        let resid = named_list_elt(obj, "residuals");
        if resid.is_null() || resid == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(resid) as usize;
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, 2);
        let _m = protect(mat);
        for i in 0..n {
            *REAL(mat).add(i) = 1.0;
            *REAL(mat).add(i + n) = (i + 1) as f64;
        }
        mat
    }
}

/// GNU `reformulate(termlabels, response=NULL)` — build a formula.
pub unsafe fn do_reformulate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let labels = CAR(args);
        if labels.is_null() || labels == R_NilValue() || TYPEOF(labels) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        let n = XLENGTH(labels);
        if n <= 0 {
            return R_NilValue();
        }
        let plus = crate::sexp::symbol::Rf_install(c"+".as_ptr());
        let mut rhs = R_NilValue();
        for i in 0..n {
            let raw = CHAR(STRING_ELT(labels, i));
            if raw.is_null() {
                continue;
            }
            let lab = std::ffi::CStr::from_ptr(raw).to_string_lossy();
            let term = if lab.as_ref() == "1" {
                Rf_ScalarInteger(1)
            } else {
                let c = std::ffi::CString::new(lab.as_ref()).unwrap_or_default();
                crate::sexp::symbol::Rf_install(c.as_ptr())
            };
            rhs = if rhs == R_NilValue() {
                term
            } else {
                let node = Rf_lang3(plus, rhs, term);
                let _n = protect(node);
                node
            };
        }
        if rhs == R_NilValue() {
            return R_NilValue();
        }
        let tilde = crate::sexp::symbol::Rf_install(c"~".as_ptr());
        let resp = CAR(CDR(args));
        let form = if resp.is_null() || resp == R_NilValue() {
            Rf_lang2(tilde, rhs)
        } else if TYPEOF(resp) == SEXPTYPE::STRSXP && XLENGTH(resp) > 0 {
            let raw = CHAR(STRING_ELT(resp, 0));
            let lab = std::ffi::CStr::from_ptr(raw).to_string_lossy();
            let c = std::ffi::CString::new(lab.as_ref()).unwrap_or_default();
            Rf_lang3(tilde, crate::sexp::symbol::Rf_install(c.as_ptr()), rhs)
        } else {
            Rf_lang3(tilde, resp, rhs)
        };
        let _f = protect(form);
        let class = Rf_mkString(c"formula".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(form, crate::sexp::attrib_core::R_ClassSymbol(), class);
        form
    }
}

fn sexp_is_numeric_zero(x: SEXP) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        if TYPEOF(x) == SEXPTYPE::INTSXP && XLENGTH(x) == 1 {
            return *INTEGER(x) == 0;
        }
        if TYPEOF(x) == SEXPTYPE::REALSXP && XLENGTH(x) == 1 {
            return *REAL(x) == 0.0;
        }
        false
    }
}

fn sexp_is_numeric_one(x: SEXP) -> bool {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::INTSXP && XLENGTH(x) == 1 {
            return *INTEGER(x) == 1;
        }
        if TYPEOF(x) == SEXPTYPE::REALSXP && XLENGTH(x) == 1 {
            return *REAL(x) == 1.0;
        }
        false
    }
}

/// GNU `is.empty.model(x)` — no terms and no intercept.
pub unsafe fn do_is_empty_model(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::LANGSXP {
            return Rf_ScalarLogical(0);
        }
        let rest = CDR(x);
        let third = if rest.is_null() {
            R_NilValue()
        } else {
            CDR(rest)
        };
        let rhs = if third.is_null() || third == R_NilValue() {
            if rest.is_null() {
                R_NilValue()
            } else {
                CAR(rest)
            }
        } else {
            CAR(third)
        };
        let empty = if sexp_is_numeric_zero(rhs) {
            true
        } else if TYPEOF(rhs) == SEXPTYPE::LANGSXP {
            let op = CAR(rhs);
            let name = if !op.is_null() && TYPEOF(op) == SEXPTYPE::SYMSXP {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(op)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            name == "-" && sexp_is_numeric_one(CADR(rhs))
        } else {
            false
        };
        Rf_ScalarLogical(if empty { 1 } else { 0 })
    }
}

/// GNU `alias.lm` for full-rank models — `list(Model = formula)`.
pub unsafe fn do_alias(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let call = named_list_elt(obj, "call");
        let model = if !call.is_null() && call != R_NilValue() && TYPEOF(call) == SEXPTYPE::LANGSXP
        {
            CADR(call)
        } else {
            R_NilValue()
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, model);
        let names = Rf_mkString(c"Model".as_ptr());
        let _nm = protect(names);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        let class = Rf_mkString(c"listof".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn term_label_matches(column: &str, label: &str) -> bool {
    let compact = |s: &str| {
        s.trim_matches('`')
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
    };
    compact(column) == compact(label)
}

/// GNU `.External2(C_modelmatrix, t, data)` for intercept + numeric terms.
pub unsafe fn modelmatrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let args = CDR(args);
        let terms = CAR(args);
        let data = CADR(args);
        if data.is_null() || TYPEOF(data) != SEXPTYPE::VECSXP || XLENGTH(data) == 0 {
            crate::main::errors::Rf_error(
                b"invalid model frame\0".as_ptr() as *const std::os::raw::c_char
            );
        }
        let first = VECTOR_ELT(data, 0);
        let dim0 =
            crate::sexp::attrib_core::getAttrib(first, crate::sexp::attrib_core::R_DimSymbol());
        let n = if !dim0.is_null()
            && dim0 != R_NilValue()
            && TYPEOF(dim0) == SEXPTYPE::INTSXP
            && XLENGTH(dim0) >= 2
        {
            *INTEGER(dim0) as i64
        } else {
            XLENGTH(first)
        };
        let intercept_attr = crate::sexp::attrib_core::getAttrib(
            terms,
            crate::sexp::symbol::Rf_install(c"intercept".as_ptr()),
        );
        let intercept = if intercept_attr.is_null() || intercept_attr == R_NilValue() {
            1
        } else if TYPEOF(intercept_attr) == SEXPTYPE::INTSXP
            || TYPEOF(intercept_attr) == SEXPTYPE::LGLSXP
        {
            *INTEGER(intercept_attr)
        } else if TYPEOF(intercept_attr) == SEXPTYPE::REALSXP {
            *REAL(intercept_attr) as i32
        } else {
            1
        };
        let labs = crate::sexp::attrib_core::getAttrib(
            terms,
            crate::sexp::symbol::Rf_install(c"term.labels".as_ptr()),
        );
        let nterms = if labs.is_null() || TYPEOF(labs) != SEXPTYPE::STRSXP {
            0
        } else {
            XLENGTH(labs)
        };
        let mut term_cols: Vec<Vec<Vec<f64>>> = Vec::new();
        let mut term_names: Vec<Vec<String>> = Vec::new();
        let names_for_width =
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_NamesSymbol());
        // GNU TermCode (model.c): inside term j, a factor part is coded by
        // full indicators (factors entry 2) when dropping it leaves other
        // variables not covered by any earlier term. Additionally, without an
        // intercept the first factor found (term order, then variable order)
        // is forced to full indicators.
        let term_parts: Vec<Vec<String>> = (0..nterms)
            .map(|j| {
                let lab = if TYPEOF(labs) == SEXPTYPE::STRSXP {
                    std::ffi::CStr::from_ptr(CHAR(STRING_ELT(labs, j)))
                        .to_string_lossy()
                        .into_owned()
                } else {
                    String::new()
                };
                split_top_level_colon(&lab)
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            })
            .collect();
        let mut full_map: Vec<Vec<bool>> = term_parts
            .iter()
            .enumerate()
            .map(|(j, parts)| {
                (0..parts.len())
                    .map(|k| {
                        let rest: Vec<&String> = parts
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| *i != k)
                            .map(|(_, p)| p)
                            .collect();
                        if rest.is_empty() {
                            return false;
                        }
                        !(0..j).any(|t| rest.iter().all(|p| term_parts[t].iter().any(|q| q == *p)))
                    })
                    .collect()
            })
            .collect();
        if intercept == 0 {
            'adjust: for (j, parts) in term_parts.iter().enumerate() {
                for (k, part) in parts.iter().enumerate() {
                    if let Some(colx) = find_data_column(data, part) {
                        if is_factor_column(colx) {
                            full_map[j][k] = true;
                            break 'adjust;
                        }
                    }
                }
            }
        }
        // GNU model.c: accumulate each term's column count in a double and
        // error before allocation when it overflows INT_MAX ("term %d would
        // require %.0g columns", then "matrix would require %.0g columns").
        let mut dnc: f64 = 0.0;
        for (j, parts) in term_parts.iter().enumerate() {
            let mut dk: f64 = 1.0;
            for (k, part) in parts.iter().enumerate() {
                let Some(colx) = find_data_column(data, part) else {
                    continue;
                };
                let levels = crate::sexp::attrib_core::getAttrib(
                    colx,
                    crate::sexp::symbol::Rf_install(c"levels".as_ptr()),
                );
                if TYPEOF(levels) == SEXPTYPE::STRSXP && XLENGTH(levels) > 0 {
                    let nlev = XLENGTH(levels) as f64;
                    dk *= if full_map[j][k] {
                        nlev
                    } else {
                        (nlev - 1.0).max(1.0)
                    };
                }
            }
            if dk > i32::MAX as f64 {
                let shown = format!("{:.0e}", dk).replace('e', "e+");
                let msg = std::ffi::CString::new(format!(
                    "term {} would require {} columns",
                    j + 1,
                    shown
                ))
                .unwrap_or_default();
                crate::main::errors::Rf_error(msg.as_ptr());
            }
            dnc += dk;
        }
        if dnc > i32::MAX as f64 {
            let shown = format!("{:.0e}", dnc).replace('e', "e+");
            let msg = std::ffi::CString::new(format!("matrix would require {shown} columns"))
                .unwrap_or_default();
            crate::main::errors::Rf_error(msg.as_ptr());
        }
        for j in 0..nterms {
            let lab = if TYPEOF(labs) == SEXPTYPE::STRSXP {
                std::ffi::CStr::from_ptr(CHAR(STRING_ELT(labs, j)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if is_formula_interaction(&lab) {
                term_cols.push(interaction_product_columns(
                    data,
                    &lab,
                    n as usize,
                    &full_map[j as usize],
                ));
                term_names.push(interaction_column_names(data, &lab, &full_map[j as usize]));
                continue;
            }
            let mut colx = R_NilValue();
            if TYPEOF(names_for_width) == SEXPTYPE::STRSXP {
                for i in 0..XLENGTH(names_for_width) {
                    let nm = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names_for_width, i)))
                        .to_string_lossy()
                        .into_owned();
                    if term_label_matches(&nm, &lab) {
                        colx = VECTOR_ELT(data, i);
                        break;
                    }
                }
            }
            if !colx.is_null()
                && colx != R_NilValue()
                && (crate::mainutils::objects::inherits2(colx, c"factor".as_ptr()) != 0
                    || crate::mainutils::objects::inherits2(colx, c"ordered".as_ptr()) != 0)
            {
                let full = full_map[j as usize].first().copied().unwrap_or(false);
                term_cols.push(factor_contrast_columns(colx, full));
                term_names.push(contrast_suffixes(colx, &lab, full));
            } else if !colx.is_null() && colx != R_NilValue() && TYPEOF(colx) == SEXPTYPE::REALSXP {
                let dim = crate::sexp::attrib_core::getAttrib(
                    colx,
                    crate::sexp::attrib_core::R_DimSymbol(),
                );
                if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2 {
                    let nc = *INTEGER(dim).add(1) as usize;
                    term_cols.push(vec![Vec::new(); nc]);
                    let dn = crate::sexp::attrib_core::getAttrib(
                        colx,
                        crate::sexp::attrib_core::R_DimNamesSymbol(),
                    );
                    let colnames = if !dn.is_null()
                        && dn != R_NilValue()
                        && TYPEOF(dn) == SEXPTYPE::VECSXP
                        && XLENGTH(dn) >= 2
                    {
                        crate::sexp::accessors::VECTOR_ELT(dn, 1)
                    } else {
                        R_NilValue()
                    };
                    let names = if TYPEOF(colnames) == SEXPTYPE::STRSXP
                        && XLENGTH(colnames) as usize == nc
                    {
                        (0..nc)
                            .map(|k| {
                                let elt = crate::sexp::accessors::STRING_ELT(colnames, k as i64);
                                let text = crate::sexp::accessors::charsxp_as_utf8(elt);
                                format!("{lab}{}", String::from_utf8_lossy(&text))
                            })
                            .collect()
                    } else {
                        (1..=nc).map(|k| format!("{lab}{k}")).collect()
                    };
                    term_names.push(names);
                } else {
                    term_cols.push(Vec::new());
                    term_names.push(vec![lab.clone()]);
                }
            } else {
                term_cols.push(Vec::new());
                term_names.push(vec![lab]);
            }
        }
        let p = (if intercept != 0 { 1 } else { 0 })
            + term_cols
                .iter()
                .map(|c| if c.is_empty() { 1 } else { c.len() })
                .sum::<usize>();
        let mat =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, p as i32);
        let _m = protect(mat);
        let dst = REAL(mat);
        let mut col = 0i64;
        if intercept != 0 {
            for i in 0..n {
                *dst.add(i as usize) = 1.0;
            }
            col = 1;
        }
        let names =
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_NamesSymbol());
        for j in 0..nterms {
            let lab = if TYPEOF(labs) == SEXPTYPE::STRSXP {
                std::ffi::CStr::from_ptr(CHAR(STRING_ELT(labs, j)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            // `:` inside I(1:10) is a sequence, not an interaction.
            // An interaction is `:` or `*` outside parentheses.
            let mut depth = 0i32;
            let mut interaction = false;
            for ch in lab.chars() {
                match ch {
                    '(' | '[' => depth += 1,
                    ')' | ']' => depth = depth.saturating_sub(1),
                    ':' | '*' if depth == 0 => interaction = true,
                    _ => {}
                }
            }
            if term_cols[j as usize]
                .iter()
                .any(|column| column.len() == n as usize)
            {
                for column in &term_cols[j as usize] {
                    if column.len() != n as usize {
                        continue;
                    }
                    for i in 0..n {
                        *dst.add((i + col * n) as usize) = column[i as usize];
                    }
                    col += 1;
                }
                continue;
            }
            let mut colx = R_NilValue();
            if TYPEOF(names) == SEXPTYPE::STRSXP {
                for i in 0..XLENGTH(names) {
                    let nm = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i)))
                        .to_string_lossy()
                        .into_owned();
                    if term_label_matches(&nm, &lab) {
                        colx = VECTOR_ELT(data, i);
                        break;
                    }
                }
            }
            if !colx.is_null() && colx != R_NilValue() && TYPEOF(colx) == SEXPTYPE::REALSXP {
                let dim = crate::sexp::attrib_core::getAttrib(
                    colx,
                    crate::sexp::attrib_core::R_DimSymbol(),
                );
                if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2 {
                    let nc = *INTEGER(dim).add(1) as i64;
                    for c in 0..nc {
                        for i in 0..n {
                            *dst.add((i + col * n) as usize) =
                                *REAL(colx).add((i + c * n) as usize);
                        }
                        col += 1;
                    }
                    continue;
                }
            }
            if colx.is_null() || colx == R_NilValue() {
                crate::main::errors::Rf_error(
                    b"C_modelmatrix cannot find a numeric column for a term\0".as_ptr()
                        as *const std::os::raw::c_char,
                );
            }
            if !term_cols[j as usize].is_empty() {
                for column in &term_cols[j as usize] {
                    for i in 0..n {
                        *dst.add((i + col * n) as usize) = column[i as usize];
                    }
                    col += 1;
                }
                continue;
            }
            let col_ty = TYPEOF(colx);
            if col_ty != SEXPTYPE::REALSXP
                && col_ty != SEXPTYPE::INTSXP
                && col_ty != SEXPTYPE::LGLSXP
            {
                crate::main::errors::Rf_error(
                    b"C_modelmatrix needs numeric term columns\0".as_ptr()
                        as *const std::os::raw::c_char,
                );
            }
            for i in 0..n {
                let v = if colx.is_null() || colx == R_NilValue() {
                    0.0
                } else if TYPEOF(colx) == SEXPTYPE::REALSXP {
                    *REAL(colx).add(i as usize)
                } else if TYPEOF(colx) == SEXPTYPE::INTSXP {
                    let iv = *INTEGER(colx).add(i as usize);
                    if iv == NA_INTEGER {
                        f64::NAN
                    } else {
                        iv as f64
                    }
                } else {
                    0.0
                };
                *dst.add((i + col * n) as usize) = v;
            }
            col += 1;
        }
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, p as i64);
        let _cn = protect(cn);
        let mut c = 0i64;
        if intercept != 0 {
            SET_STRING_ELT(cn, 0, Rf_mkChar(c"(Intercept)".as_ptr()));
            c = 1;
        }
        for names in &term_names {
            for name in names {
                if c >= p as i64 {
                    break;
                }
                let cstr = std::ffi::CString::new(name.as_str()).unwrap_or_default();
                SET_STRING_ELT(cn, c, Rf_mkChar(cstr.as_ptr()));
                c += 1;
            }
        }
        let rn = character_row_names(
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_RowNamesSymbol()),
            n,
        );
        let _rn = protect(rn);
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(mat, crate::sexp::attrib_core::R_DimNamesSymbol(), dn);
        let assign = Rf_allocVector3(SEXPTYPE::INTSXP, p as i64);
        let _as = protect(assign);
        let mut a = 0i64;
        if intercept != 0 {
            *INTEGER(assign) = 0;
            a = 1;
        }
        for j in 0..nterms {
            let width = if term_cols[j as usize].is_empty() {
                1
            } else {
                term_cols[j as usize].len()
            };
            for _ in 0..width {
                *INTEGER(assign).add(a as usize) = (j + 1) as i32;
                a += 1;
            }
        }
        crate::sexp::attrib_core::setAttrib(
            mat,
            crate::sexp::symbol::Rf_install(c"assign".as_ptr()),
            assign,
        );
        mat
    }
}

/// GNU stats/R/contrast.R + main/model.c: resolve a factor's contrast
/// matrix. The "contrasts" attribute may be a matrix (used as-is) or a
/// contrast-function name (resolved and called with the levels); when
/// unset, the default from `getOption("contrasts")` applies (ordered
/// factors take the second entry). Returns a protected matrix.
unsafe fn resolve_contrast_matrix(colx: SEXP) -> Option<SEXP> {
    unsafe {
        let assigned = crate::sexp::attrib_core::getAttrib(
            colx,
            crate::sexp::symbol::Rf_install(c"contrasts".as_ptr()),
        );
        let is_matrix = !assigned.is_null()
            && assigned != R_NilValue()
            && (TYPEOF(assigned) == SEXPTYPE::REALSXP || TYPEOF(assigned) == SEXPTYPE::INTSXP)
            && {
                let dim = crate::sexp::attrib_core::getAttrib(
                    assigned,
                    crate::sexp::attrib_core::R_DimSymbol(),
                );
                !dim.is_null()
                    && dim != R_NilValue()
                    && TYPEOF(dim) == SEXPTYPE::INTSXP
                    && XLENGTH(dim) == 2
            };
        if is_matrix {
            return Some(assigned);
        }
        let name = if !assigned.is_null()
            && assigned != R_NilValue()
            && TYPEOF(assigned) == SEXPTYPE::STRSXP
            && XLENGTH(assigned) > 0
        {
            crate::mainutils::essentials::elt_to_string(assigned, 0)
        } else {
            let contrasts_opt = crate::mainutils::options::GetOption1(
                crate::sexp::symbol::Rf_install(c"contrasts".as_ptr()),
            );
            if contrasts_opt.is_null()
                || contrasts_opt == R_NilValue()
                || TYPEOF(contrasts_opt) != SEXPTYPE::STRSXP
            {
                return None;
            }
            let ordered = crate::mainutils::objects::inherits2(colx, c"ordered".as_ptr()) != 0;
            let idx = if ordered { 1 } else { 0 };
            if XLENGTH(contrasts_opt) <= idx {
                return None;
            }
            crate::mainutils::essentials::elt_to_string(contrasts_opt, idx)
        };
        if name.is_empty() {
            return None;
        }
        let levels =
            crate::sexp::attrib_core::getAttrib(colx, crate::sexp::attrib_core::R_LevelsSymbol());
        if levels.is_null() || levels == R_NilValue() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return None;
        }
        let nlev = XLENGTH(levels) as i32;
        if nlev < 2 {
            return None;
        }
        let n_s = Rf_ScalarInteger(nlev);
        let _ns = protect(n_s);
        let targs = Rf_cons(n_s, R_NilValue());
        let _ta = protect(targs);
        // Dispatch natively to the built-in contrast generators (no R
        // closure evaluation: this runs inside model-matrix construction
        // where arbitrary evaluation contexts are unavailable).
        let mat = match name.as_str() {
            "contr.treatment" => Some(do_contr_treatment(
                R_NilValue(),
                R_NilValue(),
                targs,
                R_NilValue(),
            )),
            "contr.helmert" => Some(do_contr_helmert(
                R_NilValue(),
                R_NilValue(),
                targs,
                R_NilValue(),
            )),
            "contr.sum" => Some(do_contr_sum(
                R_NilValue(),
                R_NilValue(),
                targs,
                R_NilValue(),
            )),
            "contr.poly" => Some(do_contr_poly(
                R_NilValue(),
                R_NilValue(),
                targs,
                R_NilValue(),
            )),
            "contr.SAS" => Some(do_contr_sas(
                R_NilValue(),
                R_NilValue(),
                targs,
                R_NilValue(),
            )),
            _ => None,
        };
        let result = mat?;
        let _guard = protect(result);
        // GNU contrast functions are called with the level names, so their
        // dimnames carry them; mirror that here for the native generators,
        // which only see the level count.
        let dn = crate::sexp::attrib_core::getAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
        );
        if !dn.is_null() && dn != R_NilValue() && TYPEOF(dn) == SEXPTYPE::VECSXP && XLENGTH(dn) == 2
        {
            SET_VECTOR_ELT(dn, 0, levels);
            if name == "contr.treatment" {
                let dim = crate::sexp::attrib_core::getAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimSymbol(),
                );
                if !dim.is_null() && TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2 {
                    let nc = *INTEGER(dim).add(1) as i64;
                    let cn = Rf_allocVector3(SEXPTYPE::STRSXP, nc);
                    let _cn = protect(cn);
                    for c in 0..nc {
                        SET_STRING_ELT(cn, c, STRING_ELT(levels, c + 1));
                    }
                    SET_VECTOR_ELT(dn, 1, cn);
                }
            }
        }
        // The caller must protect the result before further allocation.
        Some(result)
    }
}

/// Contrast columns for a factor. An assigned `contrasts` matrix is used
/// as-is; otherwise treatment contrasts drop the first level.
fn factor_contrast_columns(colx: SEXP, full: bool) -> Vec<Vec<f64>> {
    unsafe {
        let n = XLENGTH(colx) as usize;
        let codes: Vec<i32> = (0..n).map(|i| *INTEGER(colx).add(i)).collect();
        let assigned = crate::sexp::attrib_core::getAttrib(
            colx,
            crate::sexp::symbol::Rf_install(c"contrasts".as_ptr()),
        );
        if full {
            // GNU: without an intercept, the first term's leading factor is
            // coded by full indicators (all levels), dropping nothing.
            let levels = crate::sexp::attrib_core::getAttrib(
                colx,
                crate::sexp::symbol::Rf_install(c"levels".as_ptr()),
            );
            let nlev = if TYPEOF(levels) == SEXPTYPE::STRSXP && XLENGTH(levels) > 0 {
                XLENGTH(levels) as usize
            } else {
                codes.iter().copied().filter(|c| *c > 0).max().unwrap_or(1) as usize
            };
            return (1..=nlev)
                .map(|lev| {
                    codes
                        .iter()
                        .map(|&c| if c == lev as i32 { 1.0 } else { 0.0 })
                        .collect()
                })
                .collect();
        }
        // An explicit (non-default) "contrasts" attribute — a matrix or a
        // contrast-function name — is resolved and used as-is.
        if !assigned.is_null()
            && assigned != R_NilValue()
            && let Some(assigned) = resolve_contrast_matrix(colx)
        {
            let _guard = protect(assigned);
            if assigned != R_NilValue() {
                let dim = crate::sexp::attrib_core::getAttrib(
                    assigned,
                    crate::sexp::attrib_core::R_DimSymbol(),
                );
                if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2 {
                    let nr = *INTEGER(dim) as usize;
                    let nc = *INTEGER(dim).add(1) as usize;
                    let mut cols = Vec::with_capacity(nc);
                    for c in 0..nc {
                        let mut column = vec![0.0; n];
                        for (i, &code) in codes.iter().enumerate() {
                            if code >= 1 && (code as usize) <= nr {
                                let v = if TYPEOF(assigned) == SEXPTYPE::REALSXP {
                                    *REAL(assigned).add((code as usize - 1) + c * nr)
                                } else {
                                    *INTEGER(assigned).add((code as usize - 1) + c * nr) as f64
                                };
                                column[i] = v;
                            }
                        }
                        cols.push(column);
                    }
                    return cols;
                }
            }
        }
        if !assigned.is_null()
            && assigned != R_NilValue()
            && (TYPEOF(assigned) == SEXPTYPE::REALSXP || TYPEOF(assigned) == SEXPTYPE::INTSXP)
        {
            let dim = crate::sexp::attrib_core::getAttrib(
                assigned,
                crate::sexp::attrib_core::R_DimSymbol(),
            );
            if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2 {
                let nr = *INTEGER(dim) as usize;
                let nc = *INTEGER(dim).add(1) as usize;
                let mut cols = Vec::with_capacity(nc);
                for c in 0..nc {
                    let mut column = vec![0.0; n];
                    for (i, &code) in codes.iter().enumerate() {
                        if code >= 1 && (code as usize) <= nr {
                            let v = if TYPEOF(assigned) == SEXPTYPE::REALSXP {
                                *REAL(assigned).add((code as usize - 1) + c * nr)
                            } else {
                                *INTEGER(assigned).add((code as usize - 1) + c * nr) as f64
                            };
                            column[i] = v;
                        }
                    }
                    cols.push(column);
                }
                return cols;
            }
        }
        let levels = crate::sexp::attrib_core::getAttrib(
            colx,
            crate::sexp::symbol::Rf_install(c"levels".as_ptr()),
        );
        let nlev = if TYPEOF(levels) == SEXPTYPE::STRSXP && XLENGTH(levels) > 0 {
            XLENGTH(levels) as usize
        } else {
            codes.iter().copied().filter(|c| *c > 0).max().unwrap_or(1) as usize
        };
        let mut cols = Vec::new();
        for lev in 2..=nlev {
            cols.push(
                codes
                    .iter()
                    .map(|&c| if c == lev as i32 { 1.0 } else { 0.0 })
                    .collect(),
            );
        }
        cols
    }
}

fn interaction_product_columns(
    data: SEXP,
    lab: &str,
    n: usize,
    full_flags: &[bool],
) -> Vec<Vec<f64>> {
    unsafe {
        let mut parts = Vec::new();
        let mut depth = 0i32;
        let mut start = 0usize;
        for (i, ch) in lab.char_indices() {
            match ch {
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                ':' if depth == 0 => {
                    parts.push(&lab[start..i]);
                    start = i + ch.len_utf8();
                }
                _ => {}
            }
        }
        parts.push(&lab[start..]);
        let names =
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_NamesSymbol());
        let mut groups: Vec<Vec<Vec<f64>>> = Vec::new();
        for (part_index, part) in parts.iter().enumerate() {
            let mut colx = R_NilValue();
            if TYPEOF(names) == SEXPTYPE::STRSXP {
                for i in 0..XLENGTH(names) {
                    let nm = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i)))
                        .to_string_lossy()
                        .into_owned();
                    if term_label_matches(&nm, part) {
                        colx = VECTOR_ELT(data, i);
                        break;
                    }
                }
            }
            if colx.is_null() || colx == R_NilValue() {
                return Vec::new();
            }
            if crate::mainutils::objects::inherits2(colx, c"factor".as_ptr()) != 0
                || crate::mainutils::objects::inherits2(colx, c"ordered".as_ptr()) != 0
            {
                let full = full_flags.get(part_index).copied().unwrap_or(false);
                groups.push(factor_contrast_columns(colx, full));
            } else {
                let mut column = vec![0.0; n];
                for i in 0..n {
                    column[i] = crate::mainutils::essentials::elt_real_safe(colx, i as i64);
                }
                groups.push(vec![column]);
            }
        }
        let mut products = vec![vec![1.0; n]];
        for group in &groups {
            if group.is_empty() {
                return Vec::new();
            }
            let mut next = Vec::new();
            for base in &products {
                for column in group {
                    let mut product = base.clone();
                    for i in 0..n {
                        product[i] *= column[i];
                    }
                    next.push(product);
                }
            }
            products = next;
        }
        products
    }
}

fn contrast_suffixes(colx: SEXP, prefix: &str, full: bool) -> Vec<String> {
    unsafe {
        let assigned = crate::sexp::attrib_core::getAttrib(
            colx,
            crate::sexp::symbol::Rf_install(c"contrasts".as_ptr()),
        );
        // An explicit (non-default) "contrasts" attribute resolves through
        // the contrast function when given by name; names come from the
        // resulting matrix dimnames.
        if !full
            && !assigned.is_null()
            && assigned != R_NilValue()
            && let Some(assigned) = resolve_contrast_matrix(colx)
        {
            let _guard = protect(assigned);
            if assigned != R_NilValue() {
                let dim = crate::sexp::attrib_core::getAttrib(
                    assigned,
                    crate::sexp::attrib_core::R_DimSymbol(),
                );
                if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2 {
                    let nc = *INTEGER(dim).add(1) as i64;
                    let dn = crate::sexp::attrib_core::getAttrib(
                        assigned,
                        crate::sexp::attrib_core::R_DimNamesSymbol(),
                    );
                    let colnames =
                        if !dn.is_null() && TYPEOF(dn) == SEXPTYPE::VECSXP && XLENGTH(dn) >= 2 {
                            VECTOR_ELT(dn, 1)
                        } else {
                            R_NilValue()
                        };
                    let mut out = Vec::new();
                    for c in 0..nc {
                        let suffix =
                            if TYPEOF(colnames) == SEXPTYPE::STRSXP && c < XLENGTH(colnames) {
                                crate::mainutils::essentials::elt_to_string(colnames, c)
                            } else {
                                (c + 1).to_string()
                            };
                        out.push(format!("{prefix}{suffix}"));
                    }
                    return out;
                }
            }
        }
        if !assigned.is_null()
            && assigned != R_NilValue()
            && (TYPEOF(assigned) == SEXPTYPE::REALSXP || TYPEOF(assigned) == SEXPTYPE::INTSXP)
        {
            let dim = crate::sexp::attrib_core::getAttrib(
                assigned,
                crate::sexp::attrib_core::R_DimSymbol(),
            );
            if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2 {
                let nc = *INTEGER(dim).add(1) as i64;
                let dn = crate::sexp::attrib_core::getAttrib(
                    assigned,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                );
                let colnames =
                    if !dn.is_null() && TYPEOF(dn) == SEXPTYPE::VECSXP && XLENGTH(dn) >= 2 {
                        VECTOR_ELT(dn, 1)
                    } else {
                        R_NilValue()
                    };
                let mut out = Vec::new();
                for c in 0..nc {
                    let suffix = if TYPEOF(colnames) == SEXPTYPE::STRSXP && c < XLENGTH(colnames) {
                        crate::mainutils::essentials::elt_to_string(colnames, c)
                    } else {
                        (c + 1).to_string()
                    };
                    out.push(format!("{prefix}{suffix}"));
                }
                return out;
            }
        }
        let levels =
            crate::sexp::attrib_core::getAttrib(colx, crate::sexp::attrib_core::R_LevelsSymbol());
        let mut out = Vec::new();
        if TYPEOF(levels) == SEXPTYPE::STRSXP {
            let from = if full { 0 } else { 1 };
            for i in from..XLENGTH(levels) {
                out.push(format!(
                    "{prefix}{}",
                    crate::mainutils::essentials::elt_to_string(levels, i)
                ));
            }
        }
        out
    }
}

unsafe fn find_data_column(data: SEXP, label: &str) -> Option<SEXP> {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_NamesSymbol());
        if TYPEOF(names) != SEXPTYPE::STRSXP {
            return None;
        }
        for i in 0..XLENGTH(names) {
            let nm = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i)))
                .to_string_lossy()
                .into_owned();
            if term_label_matches(&nm, label) {
                return Some(VECTOR_ELT(data, i));
            }
        }
        None
    }
}

unsafe fn is_factor_column(colx: SEXP) -> bool {
    unsafe {
        !colx.is_null()
            && colx != R_NilValue()
            && (crate::mainutils::objects::inherits2(colx, c"factor".as_ptr()) != 0
                || crate::mainutils::objects::inherits2(colx, c"ordered".as_ptr()) != 0)
    }
}

fn interaction_column_names(data: SEXP, lab: &str, full_flags: &[bool]) -> Vec<String> {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_NamesSymbol());
        let mut groups = Vec::new();
        let mut width = 1.0f64;
        for (part_index, part) in split_top_level_colon(lab).iter().enumerate() {
            let mut colx = R_NilValue();
            if TYPEOF(names) == SEXPTYPE::STRSXP {
                for i in 0..XLENGTH(names) {
                    let nm = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i)))
                        .to_string_lossy()
                        .into_owned();
                    if term_label_matches(&nm, part) {
                        colx = VECTOR_ELT(data, i);
                        break;
                    }
                }
            }
            if colx.is_null() || colx == R_NilValue() {
                return Vec::new();
            }
            if crate::mainutils::objects::inherits2(colx, c"factor".as_ptr()) != 0
                || crate::mainutils::objects::inherits2(colx, c"ordered".as_ptr()) != 0
            {
                let full = full_flags.get(part_index).copied().unwrap_or(false);
                let suffixes = contrast_suffixes(colx, part, full);
                groups.push(suffixes);
            } else {
                groups.push(vec![part.to_string()]);
            }
        }
        if width > i32::MAX as f64 {
            let shown = format!("{width:.0e}").replace('e', "e+");
            let msg = std::ffi::CString::new(format!("term 1 would require {shown} columns"))
                .unwrap_or_default();
            crate::main::errors::Rf_error(msg.as_ptr());
        }
        let mut names_out = vec![String::new()];
        for group in &groups {
            if group.is_empty() {
                return Vec::new();
            }
            let mut next = Vec::new();
            for base in &names_out {
                for suffix in group {
                    if base.is_empty() {
                        next.push(suffix.clone());
                    } else {
                        next.push(format!("{base}:{suffix}"));
                    }
                }
            }
            names_out = next;
        }
        names_out
    }
}

/// GNU `contr.treatment(n, base=1)`.
pub unsafe fn do_contr_treatment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if TYPEOF(n_s) == SEXPTYPE::INTSXP {
            *INTEGER(n_s)
        } else {
            *REAL(n_s) as c_int
        };
        let mut base = 1i32;
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let b = CAR(rest);
            if TYPEOF(b) == SEXPTYPE::INTSXP {
                base = *INTEGER(b);
            } else if TYPEOF(b) == SEXPTYPE::REALSXP {
                base = *REAL(b) as c_int;
            }
        }
        if n < 2 || base < 1 || base > n {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid contrasts",
            );
        }
        let nc = (n - 1) as i32;
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n, nc);
        let _m = protect(mat);
        for i in 0..(n as usize * nc as usize) {
            *REAL(mat).add(i) = 0.0;
        }
        let mut col = 0usize;
        for lev in 1..=n {
            if lev == base {
                continue;
            }
            *REAL(mat).add((lev as usize - 1) + col * n as usize) = 1.0;
            col += 1;
        }
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, n as i64);
        let _rn = protect(rn);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, nc as i64);
        let _cn = protect(cn);
        for i in 0..n as usize {
            let lab = format!("{}\0", i + 1);
            SET_STRING_ELT(rn, i as i64, Rf_mkChar(lab.as_ptr() as *const _));
        }
        col = 0;
        for lev in 1..=n {
            if lev == base {
                continue;
            }
            let lab = format!("{lev}\0");
            SET_STRING_ELT(cn, col as i64, Rf_mkChar(lab.as_ptr() as *const _));
            col += 1;
        }
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(mat, crate::sexp::attrib_core::R_DimNamesSymbol(), dn);
        mat
    }
}

/// GNU `contr.sum(n)`.
pub unsafe fn do_contr_sum(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if TYPEOF(n_s) == SEXPTYPE::INTSXP {
            *INTEGER(n_s)
        } else {
            *REAL(n_s) as c_int
        };
        if n < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid contrasts",
            );
        }
        let nc = n - 1;
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n, nc);
        let _m = protect(mat);
        for i in 0..(n as usize * nc as usize) {
            *REAL(mat).add(i) = 0.0;
        }
        for j in 0..nc as usize {
            *REAL(mat).add(j + j * n as usize) = 1.0;
            *REAL(mat).add((n as usize - 1) + j * n as usize) = -1.0;
        }
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, n as i64);
        let _rn = protect(rn);
        for i in 0..n as usize {
            let lab = format!("{}\0", i + 1);
            SET_STRING_ELT(rn, i as i64, Rf_mkChar(lab.as_ptr() as *const _));
        }
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, R_NilValue());
        crate::sexp::attrib_core::setAttrib(mat, crate::sexp::attrib_core::R_DimNamesSymbol(), dn);
        mat
    }
}

/// GNU `contr.helmert(n)`.
pub unsafe fn do_contr_helmert(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if TYPEOF(n_s) == SEXPTYPE::INTSXP {
            *INTEGER(n_s)
        } else {
            *REAL(n_s) as c_int
        };
        if n < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid contrasts",
            );
        }
        let nc = n - 1;
        let mat = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n, nc);
        let _m = protect(mat);
        for i in 0..(n as usize * nc as usize) {
            *REAL(mat).add(i) = 0.0;
        }
        for j in 0..nc as usize {
            for i in 0..=j {
                *REAL(mat).add(i + j * n as usize) = -1.0;
            }
            *REAL(mat).add((j + 1) + j * n as usize) = (j + 1) as f64;
        }
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, n as i64);
        let _rn = protect(rn);
        for i in 0..n as usize {
            let lab = format!("{}\0", i + 1);
            SET_STRING_ELT(rn, i as i64, Rf_mkChar(lab.as_ptr() as *const _));
        }
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, R_NilValue());
        crate::sexp::attrib_core::setAttrib(mat, crate::sexp::attrib_core::R_DimNamesSymbol(), dn);
        mat
    }
}

/// GNU `contr.poly(n)` — orthonormal polynomials on scores `1:n`.
pub unsafe fn do_contr_poly(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if TYPEOF(n_s) == SEXPTYPE::INTSXP {
            *INTEGER(n_s)
        } else {
            *REAL(n_s) as c_int
        } as usize;
        if n < 2 {
            return R_NilValue();
        }
        let scores: Vec<f64> = (1..=n).map(|i| i as f64).collect();
        let mean = scores.iter().sum::<f64>() / n as f64;
        let y: Vec<f64> = scores.iter().map(|s| s - mean).collect();
        let mut cols: Vec<Vec<f64>> = (0..n)
            .map(|k| y.iter().map(|v| v.powi(k as i32)).collect())
            .collect();
        for j in 0..n {
            for i in 0..j {
                let dot: f64 = cols[j].iter().zip(&cols[i]).map(|(a, b)| a * b).sum();
                let nrm: f64 = cols[i].iter().map(|a| a * a).sum();
                if nrm > 0.0 {
                    let c = dot / nrm;
                    for k in 0..n {
                        cols[j][k] -= c * cols[i][k];
                    }
                }
            }
            let nrm = cols[j].iter().map(|a| a * a).sum::<f64>().sqrt();
            if nrm > 0.0 {
                for k in 0..n {
                    cols[j][k] /= nrm;
                }
            }
            // GNU QR keeps the first nonzero entry of each contrast matching
            // the sign of the raw monomial (linear starts negative).
            if let Some(&first) = cols[j].iter().find(|v| v.abs() > 1e-12)
                && first > 0.0
                && j == 1
            {
                for k in 0..n {
                    cols[j][k] = -cols[j][k];
                }
            }
        }
        let nc = n - 1;
        let mat =
            crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, nc as i32);
        let _m = protect(mat);
        for j in 0..nc {
            for i in 0..n {
                *REAL(mat).add(i + j * n) = cols[j + 1][i];
            }
        }
        mat
    }
}

/// GNU `contr.SAS(n)` is treatment with last level as base.
pub unsafe fn do_contr_sas(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if TYPEOF(n_s) == SEXPTYPE::INTSXP {
            *INTEGER(n_s)
        } else {
            *REAL(n_s) as c_int
        };
        let base = Rf_ScalarInteger(n);
        let _b = protect(base);
        let newargs = Rf_cons(n_s, Rf_cons(base, R_NilValue()));
        let _a = protect(newargs);
        do_contr_treatment(call, op, newargs, rho)
    }
}

/// GNU `contrasts(factor)` default treatment coding.
pub unsafe fn do_contrasts(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let stored = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"contrasts".as_ptr()),
        );
        let stored_is_matrix = !stored.is_null()
            && stored != R_NilValue()
            && (TYPEOF(stored) == SEXPTYPE::REALSXP || TYPEOF(stored) == SEXPTYPE::INTSXP);
        if stored_is_matrix {
            return stored;
        }
        // GNU contrast.R: a character "contrasts" attribute (or none) is
        // resolved through the contrast function named there (or the
        // getOption default), evaluated with the factor's levels.
        if let Some(resolved) = resolve_contrast_matrix(x) {
            if resolved != R_NilValue() {
                return resolved;
            }
        }
        let levels =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_LevelsSymbol());
        let n = if !levels.is_null() && levels != R_NilValue() && TYPEOF(levels) == SEXPTYPE::STRSXP
        {
            XLENGTH(levels) as c_int
        } else {
            0
        };
        if n < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "contrasts apply only to factors with 2 or more levels",
            );
        }
        let n_s = Rf_ScalarInteger(n);
        let _ns = protect(n_s);
        let targs = Rf_cons(n_s, R_NilValue());
        let _ta = protect(targs);
        let mat = do_contr_treatment(call, op, targs, rho);
        let _m = protect(mat);
        let nc = n - 1;
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, n as i64);
        let _rn = protect(rn);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, nc as i64);
        let _cn = protect(cn);
        for i in 0..n as i64 {
            SET_STRING_ELT(rn, i, STRING_ELT(levels, i));
        }
        for i in 0..nc as i64 {
            SET_STRING_ELT(cn, i, STRING_ELT(levels, i + 1));
        }
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(mat, crate::sexp::attrib_core::R_DimNamesSymbol(), dn);
        mat
    }
}

/// GNU `contrasts(x) <- value` — store the contrast matrix.
pub unsafe fn do_contrasts_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let val = CAR(CDR(args));
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"contrasts".as_ptr()),
            val,
        );
        x
    }
}

/// GNU `C(factor)` attaches the default contrast name.
pub unsafe fn do_C(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let object = crate::mainutils::duplicate::Rf_duplicate(CAR(args));
        let ordered = crate::mainutils::objects::inherits2(object, c"ordered".as_ptr()) != FALSE;
        let (kind, name) = if ordered {
            (c"contr.poly".as_ptr(), c"ordered".as_ptr())
        } else {
            (c"contr.treatment".as_ptr(), c"unordered".as_ptr())
        };
        let contr = Rf_mkString(kind);
        let _c = protect(contr);
        let names = Rf_mkString(name);
        let _n = protect(names);
        crate::sexp::attrib_core::setAttrib(
            contr,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        crate::sexp::attrib_core::setAttrib(
            object,
            crate::sexp::symbol::Rf_install(c"contrasts".as_ptr()),
            contr,
        );
        object
    }
}

/// GNU `C_ar2ma(ar, npsi)` — MA(∞) coefficients from an AR polynomial.
pub unsafe extern "C-unwind" fn c_ar2ma(ar: SEXP, npsi: SEXP) -> SEXP {
    unsafe {
        let p = XLENGTH(ar) as usize;
        let ns = if TYPEOF(npsi) == SEXPTYPE::INTSXP {
            INTEGER(npsi).read()
        } else {
            REAL(npsi).read() as i32
        };
        let ns = ns.max(0) as usize;
        let ns1 = ns + p + 1;
        let mut psi = vec![0.0; ns1];
        for i in 0..p {
            psi[i] = if TYPEOF(ar) == SEXPTYPE::REALSXP {
                REAL(ar).add(i).read()
            } else {
                INTEGER(ar).add(i).read() as f64
            };
        }
        for i in 0..ns1.saturating_sub(p + 1) {
            for j in 0..p {
                let phi = if TYPEOF(ar) == SEXPTYPE::REALSXP {
                    REAL(ar).add(j).read()
                } else {
                    INTEGER(ar).add(j).read() as f64
                };
                psi[i + j + 1] += phi * psi[i];
            }
        }
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, ns as i64);
        for i in 0..ns {
            REAL(ans).add(i).write(psi[i]);
        }
        ans
    }
}
