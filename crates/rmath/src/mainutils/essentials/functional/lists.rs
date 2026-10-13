//! list(), expand.grid(), stack() and string-vector helpers.

use super::*;

// ---------------------------------------------------------------------------
// List / data.frame operations
// ---------------------------------------------------------------------------

/// R's `list(...)` — create a VECSXP (list) from arguments.
pub unsafe fn do_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut n: R_xlen_t = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            n += 1;
            current = CDR(current);
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let mut i: R_xlen_t = 0;
        current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            SET_VECTOR_ELT(result, i as i64, arg);
            i += 1;
            current = CDR(current);
        }
        // Copy names from the pairlist tags if present
        let mut name_parts: Vec<String> = Vec::new();
        let mut has_names = false;
        current = args;
        while !current.is_null() && current != R_NilValue() {
            let tag = crate::sexp::accessors::TAG(current);
            if !tag.is_null() && tag != R_NilValue() {
                let pname = crate::sexp::accessors::PRINTNAME(tag);
                if !pname.is_null() {
                    let s = crate::sexp::accessors::CHAR(pname);
                    if !s.is_null() {
                        name_parts.push(
                            std::ffi::CStr::from_ptr(s)
                                .to_str()
                                .unwrap_or("")
                                .to_string(),
                        );
                        has_names = true;
                    } else {
                        name_parts.push(String::new());
                    }
                } else {
                    name_parts.push(String::new());
                }
            } else {
                name_parts.push(String::new());
            }
            current = CDR(current);
        }
        if has_names {
            set_string_names(result, &name_parts);
        }
        result
    }
}

pub(crate) unsafe fn string_at_or_empty(x: SEXP, index: R_xlen_t) -> String {
    unsafe {
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP || index >= XLENGTH(x)
        {
            return String::new();
        }
        let value = STRING_ELT(x, index);
        if value.is_null() || value == crate::sexp::globals::R_NaString() {
            return String::new();
        }
        CStr::from_ptr(CHAR(value)).to_string_lossy().into_owned()
    }
}

pub(crate) unsafe fn set_string_names(x: SEXP, names: &[String]) {
    unsafe {
        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        if names_vec.is_null() {
            return;
        }
        let _names_guard = protect(names_vec);
        for (i, name) in names.iter().enumerate() {
            let cstr = CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(names_vec, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names_vec,
        );
    }
}

/// GNU `expand.grid(...)` — Cartesian product as a data.frame.
pub unsafe fn do_expand_grid(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut cols: Vec<SEXP> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut keep_out = true;
        let mut strings_as_factors = true;
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let tag_s = if !tag.is_null() && tag != R_NilValue() {
                let p = PRINTNAME(tag);
                if p.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(CHAR(p))
                        .to_string_lossy()
                        .into_owned()
                }
            } else {
                String::new()
            };
            if tag_s == "KEEP.OUT.ATTRS" {
                keep_out = crate::main::coerce::asLogical(CAR(cell)) != 0;
            } else if tag_s == "stringsAsFactors" {
                strings_as_factors = crate::main::coerce::asLogical(CAR(cell)) != 0;
            } else {
                cols.push(CAR(cell));
                names.push(tag_s);
            }
            cell = CDR(cell);
        }
        if cols.len() == 1 && TYPEOF(cols[0]) == SEXPTYPE::VECSXP {
            let lst = cols[0];
            let n = XLENGTH(lst);
            let nm =
                crate::sexp::attrib_core::getAttrib(lst, crate::sexp::attrib_core::R_NamesSymbol());
            cols = (0..n).map(|i| VECTOR_ELT(lst, i)).collect();
            names = (0..n)
                .map(|i| {
                    if !nm.is_null() && nm != R_NilValue() && TYPEOF(nm) == SEXPTYPE::STRSXP {
                        elt_to_string(nm, i)
                    } else {
                        String::new()
                    }
                })
                .collect();
        }
        let nargs = cols.len();
        if nargs == 0 {
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
            set_string_names(result, &[]);
            set_compact_row_names(result, 0);
            set_data_frame_class(result);
            return result;
        }
        for (i, name) in names.iter_mut().enumerate() {
            if name.is_empty() {
                *name = format!("Var{}", i + 1);
            }
        }
        let lens: Vec<i64> = cols.iter().map(|c| XLENGTH(*c)).collect();
        let mut total: i64 = 1;
        for &n in &lens {
            total = total.saturating_mul(n);
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, nargs as i64);
        let _result = protect(result);
        let mut rep_fac: i64 = 1;
        let mut orep = total;
        for i in 0..nargs {
            let mut x = cols[i];
            let nx = lens[i];
            orep = if nx == 0 { 0 } else { orep / nx };
            if strings_as_factors && TYPEOF(x) == SEXPTYPE::STRSXP {
                let mut uniq: Vec<String> = Vec::new();
                for k in 0..XLENGTH(x) {
                    let text = crate::mainutils::essentials::elt_to_string(x, k);
                    if !uniq.iter().any(|u| u == &text) {
                        uniq.push(text);
                    }
                }
                let lev = Rf_allocVector3(SEXPTYPE::STRSXP, uniq.len() as i64);
                let _lv = protect(lev);
                for (k, text) in uniq.iter().enumerate() {
                    let cstr = std::ffi::CString::new(text.as_str()).unwrap_or_default();
                    SET_STRING_ELT(lev, k as i64, Rf_mkChar(cstr.as_ptr()));
                }
                let fargs = Rf_cons(lev, R_NilValue());
                let fargs = Rf_cons(x, fargs);
                let _fa = protect(fargs);
                x = crate::mainutils::essentials::do_factor(_call, _op, fargs, _rho);
            }
            let expanded = expand_grid_column(x, nx, rep_fac, orep, total);
            let levels =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_LevelsSymbol());
            if !levels.is_null() && levels != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    expanded,
                    crate::sexp::attrib_core::R_LevelsSymbol(),
                    levels,
                );
            }
            let class =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
            if !class.is_null() && class != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    expanded,
                    crate::sexp::attrib_core::R_ClassSymbol(),
                    class,
                );
            }
            SET_VECTOR_ELT(result, i as i64, expanded);
            rep_fac = rep_fac.saturating_mul(nx.max(1));
        }
        set_string_names(result, &names);
        set_compact_row_names(result, total);
        set_data_frame_class(result);
        if keep_out {
            let dimv = Rf_allocVector3(SEXPTYPE::INTSXP, nargs as i64);
            for (i, n) in lens.iter().enumerate() {
                *INTEGER(dimv).add(i) = *n as c_int;
            }
            let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, nargs as i64);
            let _dn = protect(dimnames);
            for i in 0..nargs {
                let nx = lens[i];
                let labels = Rf_allocVector3(SEXPTYPE::STRSXP, nx);
                for j in 0..nx {
                    let label = format!("{}={}", names[i], format_grid_elt(cols[i], j));
                    let cstr = CString::new(label).unwrap_or_default();
                    SET_STRING_ELT(labels, j, Rf_mkChar(cstr.as_ptr()));
                }
                SET_VECTOR_ELT(dimnames, i as i64, labels);
            }
            set_string_names(dimnames, &names);
            set_string_names(dimv, &names);
            let attrs = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            SET_VECTOR_ELT(attrs, 0, dimv);
            SET_VECTOR_ELT(attrs, 1, dimnames);
            set_string_names(attrs, &["dim".to_string(), "dimnames".to_string()]);
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"out.attrs".as_ptr()), attrs);
        }
        result
    }
}

pub(super) unsafe fn expand_grid_column(
    x: SEXP,
    nx: i64,
    rep_fac: i64,
    orep: i64,
    total: i64,
) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(TYPEOF(x), total);
        let _r = protect(result);
        if nx == 0 || total == 0 {
            return result;
        }
        let mut dst = 0i64;
        for _ in 0..orep {
            for src in 0..nx {
                for _ in 0..rep_fac {
                    copy_elt(x, src, result, dst);
                    dst += 1;
                }
            }
        }
        result
    }
}

pub(super) unsafe fn format_grid_elt(x: SEXP, i: i64) -> String {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::INTSXP => format!("{}", *INTEGER(x).add(i as usize)),
            t if t == SEXPTYPE::REALSXP => {
                let v = *REAL(x).add(i as usize);
                if v.fract() == 0.0 {
                    format!("{}", v as i64)
                } else {
                    format!("{v}")
                }
            }
            t if t == SEXPTYPE::STRSXP => elt_to_string(x, i),
            t if t == SEXPTYPE::LGLSXP => {
                let v = *INTEGER(x).add(i as usize);
                if v == 0 {
                    "FALSE".into()
                } else {
                    "TRUE".into()
                }
            }
            _ => elt_to_string(x, i),
        }
    }
}

/// GNU `stack.default` — list/data.frame columns to values+ind.
pub unsafe fn do_stack(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mut cols: Vec<SEXP> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        if TYPEOF(x) == SEXPTYPE::VECSXP {
            let n = XLENGTH(x);
            let nm =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            for i in 0..n {
                let col = VECTOR_ELT(x, i);
                let dim = crate::sexp::attrib_core::getAttrib(
                    col,
                    crate::sexp::attrib_core::R_DimSymbol(),
                );
                if !dim.is_null() && dim != R_NilValue() {
                    continue;
                }
                cols.push(col);
                let name = if !nm.is_null() && nm != R_NilValue() && TYPEOF(nm) == SEXPTYPE::STRSXP
                {
                    elt_to_string(nm, i)
                } else {
                    String::new()
                };
                names.push(name);
            }
        } else {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "at least one vector element is required",
            );
        }
        if cols.is_empty() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "at least one vector element is required",
            );
        }
        let mut total: i64 = 0;
        let lens: Vec<i64> = cols
            .iter()
            .map(|c| {
                let n = XLENGTH(*c);
                total += n;
                n
            })
            .collect();
        let values_ty = TYPEOF(cols[0]);
        let values = Rf_allocVector3(values_ty, total);
        let _values = protect(values);
        let ind = Rf_allocVector3(SEXPTYPE::INTSXP, total);
        let _ind = protect(ind);
        let mut dst = 0i64;
        for (i, col) in cols.iter().enumerate() {
            for j in 0..lens[i] {
                copy_elt(*col, j, values, dst);
                *INTEGER(ind).add(dst as usize) = (i as c_int) + 1;
                dst += 1;
            }
        }
        let levels = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as i64);
        for (i, name) in names.iter().enumerate() {
            let label = if name.is_empty() {
                format!("{}", i + 1)
            } else {
                name.clone()
            };
            let cstr = CString::new(label).unwrap_or_default();
            SET_STRING_ELT(levels, i as i64, Rf_mkChar(cstr.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            ind,
            crate::sexp::attrib_core::R_LevelsSymbol(),
            levels,
        );
        crate::sexp::attrib_core::setAttrib(
            ind,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"factor".as_ptr()),
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _result = protect(result);
        SET_VECTOR_ELT(result, 0, values);
        SET_VECTOR_ELT(result, 1, ind);
        set_string_names(result, &["values".to_string(), "ind".to_string()]);
        set_compact_row_names(result, total);
        set_data_frame_class(result);
        result
    }
}
