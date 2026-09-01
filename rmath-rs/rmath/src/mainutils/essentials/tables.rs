//! Essentials domain module `tables` — extracted verbatim from essentials.rs.

use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString};
use std::os::raw::c_int;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
use crate::sexp::context::RError;
use crate::sexp::ffi::{
    FALSE, ISNAN, NA_INTEGER, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Complete base R functions — table operations, factors, aggregation
// ---------------------------------------------------------------------------

/// R's `prop.table(x, margin)` — proportion table for numeric vectors and 2D matrices.
pub unsafe fn do_prop_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP {
            return x;
        }
        let n = XLENGTH(x);
        let margin = CAR(CDR(args));
        let margin_value = if margin.is_null() || margin == R_NilValue() {
            0
        } else {
            crate::mainutils::coerce::asInteger(margin)
        };

        if margin_value == 1 || margin_value == 2 {
            let dim =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
            if !dim.is_null()
                && dim != R_NilValue()
                && TYPEOF(dim) == SEXPTYPE::INTSXP
                && LENGTH(dim) == 2
            {
                let nrow = *INTEGER(dim) as R_xlen_t;
                let ncol = *INTEGER(dim).add(1) as R_xlen_t;
                let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
                if result.is_null() {
                    return R_NilValue();
                }
                let _p = protect(result);
                for col in 0..ncol {
                    for row in 0..nrow {
                        let index = row + col * nrow;
                        let denom = if margin_value == 1 {
                            (0..ncol)
                                .map(|c| numeric_value_at(x, t, row + c * nrow))
                                .sum::<f64>()
                        } else {
                            (0..nrow)
                                .map(|r| numeric_value_at(x, t, r + col * nrow))
                                .sum::<f64>()
                        };
                        *REAL(result).add(index as usize) = if denom == 0.0 {
                            numeric_value_at(x, t, index)
                        } else {
                            numeric_value_at(x, t, index) / denom
                        };
                    }
                }
                set_two_dim_attr(result, nrow, ncol);
                return result;
            }
        }

        let mut total = 0.0;
        for i in 0..n {
            total += numeric_value_at(x, t, i);
        }
        if total == 0.0 {
            return x;
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            *dst.add(i as usize) = numeric_value_at(x, t, i) / total;
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && LENGTH(dim) == 2
        {
            set_two_dim_attr(
                result,
                *INTEGER(dim) as R_xlen_t,
                *INTEGER(dim).add(1) as R_xlen_t,
            );
        }
        result
    }
}

unsafe fn numeric_value_at(x: SEXP, t: c_int, index: R_xlen_t) -> f64 {
    unsafe {
        if t == SEXPTYPE::INTSXP {
            let value = *INTEGER(x).add(index as usize);
            if value == NA_INTEGER {
                NA_REAL
            } else {
                value as f64
            }
        } else {
            *REAL(x).add(index as usize)
        }
    }
}

/// R's `addmargins(A)` — add row, column, and grand totals for 2D numeric tables.
pub unsafe fn do_addmargins(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::INTSXP && t != SEXPTYPE::REALSXP {
            return x;
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null()
            || dim == R_NilValue()
            || TYPEOF(dim) != SEXPTYPE::INTSXP
            || LENGTH(dim) != 2
        {
            return x;
        }
        let nrow = *INTEGER(dim) as R_xlen_t;
        let ncol = *INTEGER(dim).add(1) as R_xlen_t;
        if nrow < 0 || ncol < 0 {
            return x;
        }

        let out_nrow = nrow + 1;
        let out_ncol = ncol + 1;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, out_nrow * out_ncol);
        let _result_guard = protect(result);
        let out = REAL(result);
        for i in 0..out_nrow * out_ncol {
            *out.add(i as usize) = 0.0;
        }

        for col in 0..ncol {
            for row in 0..nrow {
                let src_index = row + col * nrow;
                let value = if t == SEXPTYPE::INTSXP {
                    let value = *INTEGER(x).add(src_index as usize);
                    if value == NA_INTEGER {
                        NA_REAL
                    } else {
                        value as f64
                    }
                } else {
                    *REAL(x).add(src_index as usize)
                };
                let dst_index = row + col * out_nrow;
                *out.add(dst_index as usize) = value;
            }
        }

        for row in 0..nrow {
            let mut sum = 0.0;
            for col in 0..ncol {
                sum += *out.add((row + col * out_nrow) as usize);
            }
            *out.add((row + ncol * out_nrow) as usize) = sum;
        }

        for col in 0..ncol {
            let mut sum = 0.0;
            for row in 0..nrow {
                sum += *out.add((row + col * out_nrow) as usize);
            }
            *out.add((nrow + col * out_nrow) as usize) = sum;
        }

        let mut total = 0.0;
        for row in 0..nrow {
            total += *out.add((row + ncol * out_nrow) as usize);
        }
        *out.add((nrow + ncol * out_nrow) as usize) = total;

        set_two_dim_attr(result, out_nrow, out_ncol);
        result
    }
}

/// R's `ftable(x)` — flat table.
pub unsafe fn do_ftable(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if let Some(result) = ftable_one_dim_table(x) {
            return result;
        }
        x
    }
}

unsafe fn ftable_one_dim_table(x: SEXP) -> Option<SEXP> {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null()
            || dim == R_NilValue()
            || TYPEOF(dim) != SEXPTYPE::INTSXP
            || LENGTH(dim) != 1
        {
            return None;
        }
        let n = *INTEGER(dim);
        if n < 0 || XLENGTH(x) != n as R_xlen_t {
            return None;
        }

        let value_type = TYPEOF(x);
        let result = if value_type == SEXPTYPE::INTSXP {
            let out = Rf_allocVector3(SEXPTYPE::INTSXP, n as R_xlen_t);
            if out.is_null() {
                return Some(out);
            }
            for i in 0..n as usize {
                *INTEGER(out).add(i) = 1;
            }
            out
        } else if value_type == SEXPTYPE::REALSXP {
            let out = Rf_allocVector3(SEXPTYPE::REALSXP, n as R_xlen_t);
            if out.is_null() {
                return Some(out);
            }
            for i in 0..n as usize {
                *REAL(out).add(i) = 1.0;
            }
            out
        } else {
            return None;
        };
        let _result_guard = protect(result);

        let out_dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !out_dim.is_null() {
            let _dim_guard = protect(out_dim);
            *INTEGER(out_dim) = 1;
            *INTEGER(out_dim).add(1) = n;
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimSymbol(),
                out_dim,
            );
        }

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !class.is_null() {
            let _class_guard = protect(class);
            SET_STRING_ELT(class, 0, Rf_mkChar(c"ftable".as_ptr()));
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }

        let row_vars = named_empty_list();
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"row.vars".as_ptr()), row_vars);

        let col_vars = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if !col_vars.is_null() {
            let _col_guard = protect(col_vars);
            let labels = Rf_allocVector3(SEXPTYPE::STRSXP, n as R_xlen_t);
            if !labels.is_null() {
                let _labels_guard = protect(labels);
                for i in 0..n {
                    let label = CString::new((i + 1).to_string()).unwrap_or_default();
                    SET_STRING_ELT(labels, i as R_xlen_t, Rf_mkChar(label.as_ptr()));
                }
                SET_VECTOR_ELT(col_vars, 0, labels);
            }
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
            if !names.is_null() {
                let _names_guard = protect(names);
                SET_STRING_ELT(names, 0, Rf_mkChar(c"x".as_ptr()));
                crate::sexp::attrib_core::setAttrib(
                    col_vars,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"col.vars".as_ptr()), col_vars);
        }

        Some(result)
    }
}

unsafe fn named_empty_list() -> SEXP {
    unsafe {
        let list = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        if list.is_null() {
            return R_NilValue();
        }
        let _list_guard = protect(list);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        if !names.is_null() {
            let _names_guard = protect(names);
            crate::sexp::attrib_core::setAttrib(
                list,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        list
    }
}

/// R's `xtabs(formula, data)` — cross-tabulation (simplified).
pub unsafe fn do_xtabs(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let formula = CAR(args);
        let data = CAR(CDR(args));
        if let Some(result) = xtabs_two_way_data_frame(formula, data) {
            return result;
        }
        if let Some(result) = xtabs_one_sided_vector(formula, data, rho) {
            return result;
        }
        Rf_allocVector3(SEXPTYPE::INTSXP, 0)
    }
}

unsafe fn xtabs_two_way_data_frame(formula: SEXP, data: SEXP) -> Option<SEXP> {
    unsafe {
        if formula.is_null() || formula == R_NilValue() || TYPEOF(formula) != SEXPTYPE::LANGSXP {
            return None;
        }
        if data.is_null() || data == R_NilValue() || TYPEOF(data) != SEXPTYPE::VECSXP {
            return None;
        }
        if pairlist_apply_len(formula) != 2 {
            return None;
        }
        let rhs = CADR(formula);
        if rhs.is_null() || rhs == R_NilValue() || TYPEOF(rhs) != SEXPTYPE::LANGSXP {
            return None;
        }
        if symbol_name(CAR(rhs)).as_deref() != Some("+") || pairlist_apply_len(rhs) != 3 {
            return None;
        }
        let row_expr = CADR(rhs);
        let col_expr = CAR(CDR(CDR(rhs)));
        let row_name = symbol_name(row_expr)?;
        let col_name = symbol_name(col_expr)?;
        let rows = list_element_by_name(data, &row_name)?;
        let cols = list_element_by_name(data, &col_name)?;
        if !xtabs_supported_atomic(rows) || !xtabs_supported_atomic(cols) {
            return None;
        }
        let n_obs = XLENGTH(rows);
        if XLENGTH(cols) != n_obs {
            return None;
        }

        let mut row_labels = BTreeSet::<String>::new();
        let mut col_labels = BTreeSet::<String>::new();
        for i in 0..n_obs {
            if atomic_value_is_missing(rows, i) || atomic_value_is_missing(cols, i) {
                continue;
            }
            row_labels.insert(elt_to_string(rows, i));
            col_labels.insert(elt_to_string(cols, i));
        }
        let row_labels: Vec<String> = row_labels.into_iter().collect();
        let col_labels: Vec<String> = col_labels.into_iter().collect();
        let nrow = row_labels.len() as R_xlen_t;
        let ncol = col_labels.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, nrow * ncol);
        if result.is_null() {
            return Some(result);
        }
        let _result_guard = protect(result);
        for i in 0..(nrow * ncol) as usize {
            *INTEGER(result).add(i) = 0;
        }

        let row_index: BTreeMap<String, R_xlen_t> = row_labels
            .iter()
            .enumerate()
            .map(|(i, label)| (label.clone(), i as R_xlen_t))
            .collect();
        let col_index: BTreeMap<String, R_xlen_t> = col_labels
            .iter()
            .enumerate()
            .map(|(i, label)| (label.clone(), i as R_xlen_t))
            .collect();
        for i in 0..n_obs {
            if atomic_value_is_missing(rows, i) || atomic_value_is_missing(cols, i) {
                continue;
            }
            let row = row_index.get(&elt_to_string(rows, i)).copied()?;
            let col = col_index.get(&elt_to_string(cols, i)).copied()?;
            let offset = (row + col * nrow) as usize;
            *INTEGER(result).add(offset) += 1;
        }

        set_xtabs_two_dim_metadata(
            result,
            nrow,
            ncol,
            &row_name,
            &col_name,
            &row_labels,
            &col_labels,
        );
        Some(result)
    }
}

unsafe fn xtabs_one_sided_vector(formula: SEXP, data: SEXP, rho: SEXP) -> Option<SEXP> {
    unsafe {
        if formula.is_null() || formula == R_NilValue() || TYPEOF(formula) != SEXPTYPE::LANGSXP {
            return None;
        }
        if pairlist_apply_len(formula) != 2 {
            return None;
        }
        let rhs = CADR(formula);
        if rhs.is_null() || rhs == R_NilValue() {
            return None;
        }
        let values = xtabs_resolve_rhs(rhs, data, rho);
        if values.is_null() || values == R_NilValue() {
            return None;
        }
        let value_type = TYPEOF(values);
        if value_type != SEXPTYPE::STRSXP
            && value_type != SEXPTYPE::INTSXP
            && value_type != SEXPTYPE::REALSXP
            && value_type != SEXPTYPE::LGLSXP
        {
            return None;
        }

        let mut counts = BTreeMap::<String, i32>::new();
        for i in 0..XLENGTH(values) {
            if atomic_value_is_missing(values, i) {
                continue;
            }
            let key = elt_to_string(values, i);
            *counts.entry(key).or_insert(0) += 1;
        }

        let n = counts.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return Some(result);
        }
        let _result_guard = protect(result);
        let labels = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if labels.is_null() {
            return Some(R_NilValue());
        }
        let _labels_guard = protect(labels);
        for (i, (label, count)) in counts.into_iter().enumerate() {
            *INTEGER(result).add(i) = count;
            let c_label = CString::new(label).unwrap_or_default();
            SET_STRING_ELT(labels, i as R_xlen_t, Rf_mkChar(c_label.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            labels,
        );

        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
        if !dim.is_null() {
            let _dim_guard = protect(dim);
            *INTEGER(dim) = n as c_int;
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimSymbol(),
                dim,
            );
        }

        let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if !dimnames.is_null() {
            let _dimnames_guard = protect(dimnames);
            SET_VECTOR_ELT(dimnames, 0, labels);
            let dimnames_names = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
            if !dimnames_names.is_null() {
                let _names_guard = protect(dimnames_names);
                let title = deparse_one_line(rhs);
                let c_title = CString::new(title).unwrap_or_default();
                SET_STRING_ELT(dimnames_names, 0, Rf_mkChar(c_title.as_ptr()));
                crate::sexp::attrib_core::setAttrib(
                    dimnames,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    dimnames_names,
                );
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dimnames,
            );
        }

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !class.is_null() {
            let _class_guard = protect(class);
            SET_STRING_ELT(class, 0, Rf_mkChar(c"xtabs".as_ptr()));
            SET_STRING_ELT(class, 1, Rf_mkChar(c"table".as_ptr()));
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }
        Some(result)
    }
}

unsafe fn xtabs_resolve_rhs(rhs: SEXP, data: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if !data.is_null() && data != R_NilValue() && TYPEOF(rhs) == SEXPTYPE::SYMSXP {
            if let Some(name) = symbol_name(rhs) {
                if let Some(column) = list_element_by_name(data, &name) {
                    return column;
                }
            }
        }
        crate::eval::eval::Rf_eval(rhs, rho)
    }
}

unsafe fn set_xtabs_two_dim_metadata(
    result: SEXP,
    nrow: R_xlen_t,
    ncol: R_xlen_t,
    row_name: &str,
    col_name: &str,
    row_labels: &[String],
    col_labels: &[String],
) {
    unsafe {
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            let _dim_guard = protect(dim);
            *INTEGER(dim) = nrow as c_int;
            *INTEGER(dim).add(1) = ncol as c_int;
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimSymbol(),
                dim,
            );
        }

        let row_dimnames = string_vector(row_labels);
        let _row_dimnames_guard = protect(row_dimnames);
        let col_dimnames = string_vector(col_labels);
        let _col_dimnames_guard = protect(col_dimnames);
        let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if !dimnames.is_null() {
            let _dimnames_guard = protect(dimnames);
            SET_VECTOR_ELT(dimnames, 0, row_dimnames);
            SET_VECTOR_ELT(dimnames, 1, col_dimnames);
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
            if !names.is_null() {
                let _names_guard = protect(names);
                let c_row_name = CString::new(row_name).unwrap_or_default();
                let c_col_name = CString::new(col_name).unwrap_or_default();
                SET_STRING_ELT(names, 0, Rf_mkChar(c_row_name.as_ptr()));
                SET_STRING_ELT(names, 1, Rf_mkChar(c_col_name.as_ptr()));
                crate::sexp::attrib_core::setAttrib(
                    dimnames,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dimnames,
            );
        }

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !class.is_null() {
            let _class_guard = protect(class);
            SET_STRING_ELT(class, 0, Rf_mkChar(c"xtabs".as_ptr()));
            SET_STRING_ELT(class, 1, Rf_mkChar(c"table".as_ptr()));
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }
    }
}

fn xtabs_supported_atomic(x: SEXP) -> bool {
    let value_type = unsafe { TYPEOF(x) };
    value_type == SEXPTYPE::STRSXP
        || value_type == SEXPTYPE::INTSXP
        || value_type == SEXPTYPE::REALSXP
        || value_type == SEXPTYPE::LGLSXP
}

pub(crate) unsafe fn list_element_by_name(list: SEXP, name: &str) -> Option<SEXP> {
    unsafe {
        if list.is_null() || list == R_NilValue() || TYPEOF(list) != SEXPTYPE::VECSXP {
            return None;
        }
        let names =
            crate::sexp::attrib_core::getAttrib(list, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return None;
        }
        for i in 0..XLENGTH(list) {
            if string_at_or_empty(names, i) == name {
                return Some(VECTOR_ELT(list, i));
            }
        }
        None
    }
}

unsafe fn pairlist_apply_len(x: SEXP) -> R_xlen_t {
    unsafe {
        let mut len = 0;
        let mut current = x;
        while !current.is_null() && current != R_NilValue() {
            len += 1;
            current = CDR(current);
        }
        len
    }
}

unsafe fn deparse_one_line(expr: SEXP) -> String {
    unsafe {
        let lines = crate::mainutils::deparse::deparse1(
            expr,
            false,
            crate::mainutils::deparse::DEFAULT_USER_DEPARSE,
        );
        if lines.is_null() || lines == R_NilValue() || XLENGTH(lines) == 0 {
            String::new()
        } else {
            elt_to_string(lines, 0)
        }
    }
}

/// R's `aggregate(x, by, FUN)` — aggregate numeric vectors by list columns.
pub unsafe fn do_aggregate(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let by = CAR(CDR(args));
        let fun = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if let Some(result) = aggregate_numeric_by_groups(x, by, fun, call) {
            return result;
        }
        if !fun.is_null() && fun != R_NilValue() {
            let call_args = Rf_cons(x, R_NilValue());
            let call_sexp = Rf_cons(fun, call_args);
            if !call_sexp.is_null() {
                (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            return crate::eval::eval::Rf_eval(call_sexp, rho);
        }
        x
    }
}

#[derive(Clone, Copy)]
pub(crate) enum AggregateSummary {
    Mean,
    Sum,
    Prod,
    Min,
    Max,
    Length,
    Median,
    Sd,
    Var,
    Iqr,
}

#[derive(Clone)]
pub(crate) struct AggregateGroupState {
    sum: f64,
    count: usize,
    has_na: bool,
    min: f64,
    max: f64,
    prod: f64,
    values: Vec<f64>,
}

impl AggregateGroupState {
    pub(crate) fn new() -> Self {
        Self {
            sum: 0.0,
            count: 0,
            has_na: false,
            min: 0.0,
            max: 0.0,
            prod: 1.0,
            values: Vec::new(),
        }
    }

    pub(crate) fn record(&mut self, value: f64, summary: AggregateSummary) {
        if matches!(summary, AggregateSummary::Length) {
            self.count += 1;
            return;
        }
        if value.to_bits() == R_NA_BIT_PATTERN || value.is_nan() {
            self.has_na = true;
            return;
        }
        if self.count == 0 {
            self.min = value;
            self.max = value;
        } else {
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }
        self.sum += value;
        self.prod *= value;
        self.values.push(value);
        self.count += 1;
    }

    pub(crate) fn summarize(mut self, summary: AggregateSummary) -> f64 {
        if self.has_na || self.count == 0 {
            return NA_REAL;
        }
        match summary {
            AggregateSummary::Mean => self.sum / self.count as f64,
            AggregateSummary::Sum => self.sum,
            AggregateSummary::Prod => self.prod,
            AggregateSummary::Min => self.min,
            AggregateSummary::Max => self.max,
            AggregateSummary::Length => self.count as f64,
            AggregateSummary::Median => median_value(&mut self.values),
            AggregateSummary::Sd => sample_sd_value(&self.values),
            AggregateSummary::Var => sample_variance_value(&self.values),
            AggregateSummary::Iqr => iqr_value(&mut self.values),
        }
    }
}

struct AggregateInputColumn {
    name: String,
    data: SEXP,
    data_type: c_int,
}

struct AggregateByColumn {
    data: SEXP,
    data_type: c_int,
    levels: Option<Vec<String>>,
}

#[derive(Clone)]
struct AggregateGroupValue {
    label: String,
    order_key: String,
    factor_code: Option<i32>,
}

unsafe fn aggregate_numeric_by_groups(x: SEXP, by: SEXP, fun: SEXP, call: SEXP) -> Option<SEXP> {
    unsafe {
        let summary = aggregate_summary_fun(fun, call)?;
        let input_columns = aggregate_input_columns(x)?;
        let row_count = aggregate_input_row_count(&input_columns)?;
        if by.is_null() || by == R_NilValue() || TYPEOF(by) != SEXPTYPE::VECSXP || XLENGTH(by) == 0
        {
            return None;
        }
        let mut group_columns = Vec::with_capacity(XLENGTH(by) as usize);
        for i in 0..XLENGTH(by) {
            let group = VECTOR_ELT(by, i);
            if group.is_null() || group == R_NilValue() || XLENGTH(group) != row_count {
                return None;
            }
            let group_type = TYPEOF(group);
            if group_type != SEXPTYPE::STRSXP && group_type != SEXPTYPE::INTSXP {
                return None;
            }
            group_columns.push(AggregateByColumn {
                data: group,
                data_type: group_type,
                levels: aggregate_factor_levels(group),
            });
        }

        let mut groups =
            BTreeMap::<Vec<String>, (Vec<AggregateGroupValue>, Vec<AggregateGroupState>)>::new();
        for i in 0..row_count {
            let mut labels = Vec::with_capacity(group_columns.len());
            let mut order_key = Vec::with_capacity(group_columns.len());
            for group in &group_columns {
                let value = aggregate_group_value(group, i)?;
                order_key.push(value.order_key.clone());
                labels.push(value);
            }
            order_key.reverse();
            let states = &mut groups
                .entry(order_key)
                .or_insert_with(|| {
                    (
                        labels,
                        vec![AggregateGroupState::new(); input_columns.len()],
                    )
                })
                .1;
            for (column, state) in input_columns.iter().zip(states.iter_mut()) {
                state.record(aggregate_column_value(column, i), summary);
            }
        }

        let group_count = group_columns.len() as R_xlen_t;
        let result = Rf_allocVector3(
            SEXPTYPE::VECSXP,
            group_count + input_columns.len() as R_xlen_t,
        );
        if result.is_null() {
            return Some(result);
        }
        let _result_guard = protect(result);
        let n = groups.len() as R_xlen_t;

        let mut group_result_cols = Vec::with_capacity(group_columns.len());
        for (j, group) in group_columns.iter().enumerate() {
            let group_col_type = if group.levels.is_some() {
                SEXPTYPE::INTSXP
            } else {
                SEXPTYPE::STRSXP
            };
            let group_col = Rf_allocVector3(group_col_type, n);
            if group_col.is_null() {
                return Some(R_NilValue());
            }
            let _group_guard = protect(group_col);
            if let Some(levels) = &group.levels {
                set_factor_attrs(group_col, levels);
            }
            SET_VECTOR_ELT(result, j as R_xlen_t, group_col);
            group_result_cols.push(group_col);
        }

        let mut value_cols = Vec::with_capacity(input_columns.len());
        for j in 0..input_columns.len() {
            let value_col = Rf_allocVector3(SEXPTYPE::REALSXP, n);
            if value_col.is_null() {
                return Some(R_NilValue());
            }
            let _value_guard = protect(value_col);
            SET_VECTOR_ELT(result, group_count + j as R_xlen_t, value_col);
            value_cols.push(value_col);
        }

        for (i, (_order_key, (labels, states))) in groups.into_iter().enumerate() {
            for (j, value) in labels.into_iter().enumerate() {
                if let Some(code) = value.factor_code {
                    *INTEGER(group_result_cols[j]).add(i) = code;
                } else {
                    let value_c = CString::new(value.label).unwrap_or_default();
                    SET_STRING_ELT(
                        group_result_cols[j],
                        i as R_xlen_t,
                        Rf_mkChar(value_c.as_ptr()),
                    );
                }
            }
            for (value_col, state) in value_cols.iter().zip(states.into_iter()) {
                *REAL(*value_col).add(i) = state.summarize(summary);
            }
        }

        let by_names =
            crate::sexp::attrib_core::getAttrib(by, crate::sexp::attrib_core::R_NamesSymbol());
        let mut names = Vec::with_capacity(group_columns.len() + input_columns.len());
        for i in 0..group_columns.len() {
            let group_name = if !by_names.is_null()
                && by_names != R_NilValue()
                && TYPEOF(by_names) == SEXPTYPE::STRSXP
                && XLENGTH(by_names) > i as R_xlen_t
            {
                let name = string_at_or_empty(by_names, i as R_xlen_t);
                if name.is_empty() {
                    format!("Group.{}", i + 1)
                } else {
                    name
                }
            } else {
                format!("Group.{}", i + 1)
            };
            names.push(group_name);
        }
        names.extend(input_columns.into_iter().map(|column| column.name));
        set_string_names(result, &names);
        set_compact_row_names(result, n);
        set_data_frame_class(result);
        Some(result)
    }
}

unsafe fn aggregate_input_columns(x: SEXP) -> Option<Vec<AggregateInputColumn>> {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP => {
                Some(vec![AggregateInputColumn {
                    name: "x".to_string(),
                    data: x,
                    data_type: t,
                }])
            }
            t if t == SEXPTYPE::VECSXP => {
                let names = crate::sexp::attrib_core::getAttrib(
                    x,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                );
                let mut columns = Vec::with_capacity(XLENGTH(x) as usize);
                for i in 0..XLENGTH(x) {
                    let column = VECTOR_ELT(x, i);
                    if column.is_null() || column == R_NilValue() {
                        return None;
                    }
                    let data_type = TYPEOF(column);
                    if data_type != SEXPTYPE::INTSXP && data_type != SEXPTYPE::REALSXP {
                        return None;
                    }
                    let name = if !names.is_null()
                        && names != R_NilValue()
                        && TYPEOF(names) == SEXPTYPE::STRSXP
                        && XLENGTH(names) > i
                    {
                        let candidate = string_at_or_empty(names, i);
                        if candidate.is_empty() {
                            format!("x.{}", i + 1)
                        } else {
                            candidate
                        }
                    } else {
                        format!("x.{}", i + 1)
                    };
                    columns.push(AggregateInputColumn {
                        name,
                        data: column,
                        data_type,
                    });
                }
                if columns.is_empty() {
                    None
                } else {
                    Some(columns)
                }
            }
            _ => None,
        }
    }
}

fn aggregate_input_row_count(columns: &[AggregateInputColumn]) -> Option<R_xlen_t> {
    let first = columns.first()?;
    let row_count = unsafe { XLENGTH(first.data) };
    if columns
        .iter()
        .all(|column| unsafe { XLENGTH(column.data) } == row_count)
    {
        Some(row_count)
    } else {
        None
    }
}

pub(crate) unsafe fn aggregate_factor_levels(group: SEXP) -> Option<Vec<String>> {
    unsafe {
        if TYPEOF(group) != SEXPTYPE::INTSXP {
            return None;
        }
        let levels =
            crate::sexp::attrib_core::getAttrib(group, crate::sexp::attrib_core::R_LevelsSymbol());
        if levels.is_null() || levels == R_NilValue() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return None;
        }
        Some(
            (0..XLENGTH(levels))
                .map(|i| string_at_or_empty(levels, i))
                .collect(),
        )
    }
}

pub(crate) unsafe fn set_factor_attrs(column: SEXP, levels: &[String]) {
    unsafe {
        let levels_sexp = Rf_allocVector3(SEXPTYPE::STRSXP, levels.len() as R_xlen_t);
        if levels_sexp.is_null() {
            return;
        }
        let _levels_guard = protect(levels_sexp);
        for (i, level) in levels.iter().enumerate() {
            let level_c = CString::new(level.as_str()).unwrap_or_default();
            SET_STRING_ELT(levels_sexp, i as R_xlen_t, Rf_mkChar(level_c.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            column,
            crate::sexp::attrib_core::R_LevelsSymbol(),
            levels_sexp,
        );

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if class.is_null() {
            return;
        }
        let _class_guard = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"factor".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            column,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
    }
}

unsafe fn aggregate_column_value(column: &AggregateInputColumn, i: R_xlen_t) -> f64 {
    unsafe {
        if column.data_type == SEXPTYPE::REALSXP {
            *REAL(column.data).add(i as usize)
        } else {
            let value = *INTEGER(column.data).add(i as usize);
            if value == NA_INTEGER {
                NA_REAL
            } else {
                value as f64
            }
        }
    }
}

pub(crate) unsafe fn aggregate_summary_fun(fun: SEXP, call: SEXP) -> Option<AggregateSummary> {
    unsafe {
        match call_fun_name(call).as_deref() {
            Some("median") => return Some(AggregateSummary::Median),
            Some("sd") => return Some(AggregateSummary::Sd),
            Some("var") => return Some(AggregateSummary::Var),
            Some("IQR") => return Some(AggregateSummary::Iqr),
            _ => {}
        }
        if fun.is_null() || fun == R_NilValue() {
            return Some(AggregateSummary::Mean);
        }
        let fun_type = TYPEOF(fun);
        if fun_type == SEXPTYPE::BUILTINSXP || fun_type == SEXPTYPE::SPECIALSXP {
            match crate::eval::primitive::PRIMNAME(fun) {
                "sum" => Some(AggregateSummary::Sum),
                "prod" => Some(AggregateSummary::Prod),
                "min" => Some(AggregateSummary::Min),
                "max" => Some(AggregateSummary::Max),
                "length" => Some(AggregateSummary::Length),
                "median" => Some(AggregateSummary::Median),
                "sd" => Some(AggregateSummary::Sd),
                "var" => Some(AggregateSummary::Var),
                "IQR" => Some(AggregateSummary::Iqr),
                _ => Some(AggregateSummary::Mean),
            }
        } else if fun_type == SEXPTYPE::SYMSXP {
            match symbol_name(fun).as_deref() {
                Some("sum") => Some(AggregateSummary::Sum),
                Some("prod") => Some(AggregateSummary::Prod),
                Some("min") => Some(AggregateSummary::Min),
                Some("max") => Some(AggregateSummary::Max),
                Some("length") => Some(AggregateSummary::Length),
                Some("median") => Some(AggregateSummary::Median),
                Some("sd") => Some(AggregateSummary::Sd),
                Some("var") => Some(AggregateSummary::Var),
                Some("IQR") => Some(AggregateSummary::Iqr),
                _ => Some(AggregateSummary::Mean),
            }
        } else {
            Some(AggregateSummary::Mean)
        }
    }
}

fn median_value(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return NA_REAL;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

pub(crate) fn iqr_value(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return NA_REAL;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    quantile_type7(values, 0.75) - quantile_type7(values, 0.25)
}

fn sample_sd_value(values: &[f64]) -> f64 {
    sample_variance_value(values).sqrt()
}

fn sample_variance_value(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return NA_REAL;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    values
        .iter()
        .map(|value| {
            let delta = *value - mean;
            delta * delta
        })
        .sum::<f64>()
        / (values.len() - 1) as f64
}

unsafe fn call_fun_name(call: SEXP) -> Option<String> {
    unsafe {
        if call.is_null() || call == R_NilValue() || TYPEOF(call) != SEXPTYPE::LANGSXP {
            return None;
        }
        let fun_expr = CAR(CDR(CDR(CDR(call))));
        symbol_name(fun_expr)
    }
}

unsafe fn aggregate_group_value(
    group: &AggregateByColumn,
    i: R_xlen_t,
) -> Option<AggregateGroupValue> {
    unsafe {
        if group.data_type == SEXPTYPE::STRSXP {
            let elt = STRING_ELT(group.data, i);
            if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                None
            } else {
                let label = CStr::from_ptr(CHAR(elt)).to_string_lossy().into_owned();
                Some(AggregateGroupValue {
                    order_key: label.clone(),
                    label,
                    factor_code: None,
                })
            }
        } else if group.data_type == SEXPTYPE::INTSXP {
            let value = *INTEGER(group.data).add(i as usize);
            if value == NA_INTEGER {
                None
            } else if let Some(levels) = &group.levels {
                let level_index = (value - 1) as usize;
                let label = levels.get(level_index)?.clone();
                Some(AggregateGroupValue {
                    label,
                    order_key: format!("{value:010}"),
                    factor_code: Some(value),
                })
            } else {
                let label = value.to_string();
                Some(AggregateGroupValue {
                    order_key: label.clone(),
                    label,
                    factor_code: None,
                })
            }
        } else {
            None
        }
    }
}

/// R's `ave(x, ...)` — group averages for numeric vectors using the default mean.
pub unsafe fn do_ave(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::INTSXP && t != SEXPTYPE::REALSXP {
            return x;
        }
        let n = XLENGTH(x);
        let mut group_args = Vec::new();
        let mut cursor = CDR(args);
        while !cursor.is_null() && cursor != R_NilValue() {
            let group = CAR(cursor);
            if !group.is_null() && group != R_NilValue() && XLENGTH(group) > 0 {
                group_args.push(group);
            }
            cursor = CDR(cursor);
        }
        if group_args.is_empty() {
            return x;
        }

        #[derive(Clone, Copy, Default)]
        struct AveGroup {
            sum: f64,
            count: R_xlen_t,
            has_missing: bool,
        }

        let mut groups: BTreeMap<String, AveGroup> = BTreeMap::new();
        let mut keys = Vec::with_capacity(n as usize);
        for i in 0..n {
            let key = group_args
                .iter()
                .map(|&group| elt_to_string(group, i))
                .collect::<Vec<_>>()
                .join("\r");
            let value = if t == SEXPTYPE::INTSXP {
                let raw = *INTEGER(x).add(i as usize);
                if raw == NA_INTEGER {
                    None
                } else {
                    Some(raw as f64)
                }
            } else {
                let raw = *REAL(x).add(i as usize);
                if raw.to_bits() == R_NA_BIT_PATTERN || raw.is_nan() {
                    None
                } else {
                    Some(raw)
                }
            };
            let entry = groups.entry(key.clone()).or_default();
            match value {
                Some(value) => {
                    entry.sum += value;
                    entry.count += 1;
                }
                None => entry.has_missing = true,
            }
            keys.push(key);
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _result_guard = protect(result);
        for (i, key) in keys.iter().enumerate() {
            let group = groups.get(key).copied().unwrap_or_default();
            *REAL(result).add(i) = if group.has_missing || group.count == 0 {
                NA_REAL
            } else {
                group.sum / group.count as f64
            };
        }
        result
    }
}

/// R's `by(data, INDICES, FUN)` — apply by groups (simplified).
pub unsafe fn do_by(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let data = CAR(args);
        let indices = CAR(CDR(args));
        let fun = CAR(CDR(CDR(args)));
        if data.is_null() || data == R_NilValue() {
            return R_NilValue();
        }
        if let Some(result) = tapply_numeric_array(data, indices, fun, _call) {
            set_single_class(result, "by");
            return result;
        }
        if !fun.is_null() && fun != R_NilValue() {
            let call_args = Rf_cons(data, R_NilValue());
            let call_sexp = Rf_cons(fun, call_args);
            if !call_sexp.is_null() {
                (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            return crate::eval::eval::Rf_eval(call_sexp, rho);
        }
        data
    }
}

/// R's `interaction(...)` — factor interaction (simplified).
pub unsafe fn do_interaction(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut inputs = Vec::new();
        let mut cursor = args;
        while !cursor.is_null() && cursor != R_NilValue() {
            let arg = CAR(cursor);
            if arg.is_null() || arg == R_NilValue() {
                return R_NilValue();
            }
            let arg_type = TYPEOF(arg);
            if arg_type != SEXPTYPE::STRSXP
                && arg_type != SEXPTYPE::INTSXP
                && arg_type != SEXPTYPE::REALSXP
                && arg_type != SEXPTYPE::LGLSXP
            {
                return R_NilValue();
            }
            inputs.push(arg);
            cursor = CDR(cursor);
        }
        if inputs.is_empty() {
            return R_NilValue();
        }
        interaction_factor(&inputs, ".")
    }
}

unsafe fn interaction_factor(inputs: &[SEXP], sep: &str) -> SEXP {
    unsafe {
        let n = inputs.iter().map(|arg| XLENGTH(*arg)).max().unwrap_or(0);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let input_levels = inputs
            .iter()
            .map(|arg| interaction_levels_for_arg(*arg))
            .collect::<Vec<_>>();
        if input_levels.iter().any(|levels| levels.is_empty()) {
            return R_NilValue();
        }
        let levels = interaction_cartesian_levels(&input_levels, sep);
        let level_positions = levels
            .iter()
            .enumerate()
            .map(|(i, level)| (level.clone(), i as i32 + 1))
            .collect::<BTreeMap<_, _>>();

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return result;
        }
        let _result_guard = protect(result);
        for row in 0..n {
            let mut parts = Vec::with_capacity(inputs.len());
            let mut has_na = false;
            for input in inputs {
                let input_len = XLENGTH(*input);
                if input_len == 0 {
                    has_na = true;
                    break;
                }
                let value = interaction_value_at(*input, row % input_len);
                if value.is_none() {
                    has_na = true;
                    break;
                }
                parts.push(value.unwrap_or_default());
            }
            *INTEGER(result).add(row as usize) = if has_na {
                NA_INTEGER
            } else {
                let label = parts.join(sep);
                *level_positions.get(&label).unwrap_or(&NA_INTEGER)
            };
        }
        set_factor_attrs(result, &levels);
        result
    }
}

unsafe fn interaction_levels_for_arg(arg: SEXP) -> Vec<String> {
    unsafe {
        if let Some(levels) = aggregate_factor_levels(arg) {
            return levels;
        }
        let mut levels = BTreeSet::new();
        for i in 0..XLENGTH(arg) {
            if let Some(value) = interaction_value_at(arg, i) {
                levels.insert(value);
            }
        }
        levels.into_iter().collect()
    }
}

fn interaction_cartesian_levels(levels: &[Vec<String>], sep: &str) -> Vec<String> {
    let mut labels = vec![String::new()];
    for level in levels {
        let mut next = Vec::with_capacity(labels.len() * level.len());
        for suffix in level {
            for prefix in &labels {
                if prefix.is_empty() {
                    next.push(suffix.clone());
                } else {
                    next.push(format!("{prefix}{sep}{suffix}"));
                }
            }
        }
        labels = next;
    }
    labels
}

unsafe fn interaction_value_at(arg: SEXP, i: R_xlen_t) -> Option<String> {
    unsafe {
        let arg_type = TYPEOF(arg);
        if arg_type == SEXPTYPE::STRSXP {
            let elt = STRING_ELT(arg, i);
            if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                None
            } else {
                Some(CStr::from_ptr(CHAR(elt)).to_string_lossy().into_owned())
            }
        } else if arg_type == SEXPTYPE::INTSXP || arg_type == SEXPTYPE::LGLSXP {
            let value = *INTEGER(arg).add(i as usize);
            if value == NA_INTEGER {
                None
            } else if let Some(label) = factor_label_at(arg, value) {
                Some(label)
            } else {
                Some(value.to_string())
            }
        } else if arg_type == SEXPTYPE::REALSXP {
            let value = *REAL(arg).add(i as usize);
            if value.to_bits() == R_NA_BIT_PATTERN || value.is_nan() {
                None
            } else {
                Some(value.to_string())
            }
        } else {
            None
        }
    }
}

/// R's `relevel(x, ref)` — relevel factor (simplified).
pub unsafe fn do_relevel(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // S3 dispatch rewrites the generic's call head to the method name
        // (relevel -> relevel.factor); stock R attributes errors raised
        // inside the method to that rewritten call. Duplicate the applied
        // call with its head replaced and attribute through it.
        let method_call = crate::mainutils::duplicate::duplicate(call);
        let _method_guard = protect(method_call);
        let method_sym = crate::sexp::symbol::Rf_install(c"relevel.factor".as_ptr());
        crate::sexp::accessors::SETCAR(method_call, method_sym);
        crate::mainutils::errors::attribute_handler_errors(method_call, || relevel_impl(args))
    }
}

unsafe fn relevel_impl(args: SEXP) -> SEXP {
    unsafe {
        let ref_arg = CAR(CDR(args));
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let Some(levels) = aggregate_factor_levels(x) else {
            base_error("'relevel' only for (unordered) factors");
        };
        if ref_arg.is_null() || ref_arg == R_NilValue() || XLENGTH(ref_arg) == 0 {
            base_error("'ref' must be of length one");
        }
        let ref_type = TYPEOF(ref_arg);
        let ref_pos = if ref_type == SEXPTYPE::INTSXP {
            let value = *INTEGER(ref_arg);
            if value == NA_INTEGER || value < 1 || value as usize > levels.len() {
                base_error("'ref' must be an existing level");
            }
            (value - 1) as usize
        } else if ref_type == SEXPTYPE::REALSXP {
            let value = *REAL(ref_arg);
            if value.to_bits() == R_NA_BIT_PATTERN
                || value.is_nan()
                || value < 1.0
                || value > levels.len() as f64
            {
                base_error("'ref' must be an existing level");
            }
            value as usize - 1
        } else {
            let ref_level = elt_to_string(ref_arg, 0);
            levels
                .iter()
                .position(|level| level == &ref_level)
                .unwrap_or_else(|| base_error("'ref' must be an existing level"))
        };

        let mut new_levels = Vec::with_capacity(levels.len());
        new_levels.push(levels[ref_pos].clone());
        new_levels.extend(
            levels
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != ref_pos)
                .map(|(_, level)| level.clone()),
        );
        let new_positions = new_levels
            .iter()
            .enumerate()
            .map(|(i, level)| (level.clone(), i as i32 + 1))
            .collect::<BTreeMap<_, _>>();

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, XLENGTH(x));
        if result.is_null() {
            return result;
        }
        let _result_guard = protect(result);
        for i in 0..XLENGTH(x) {
            let code = *INTEGER(x).add(i as usize);
            *INTEGER(result).add(i as usize) = if code == NA_INTEGER || code <= 0 {
                NA_INTEGER
            } else {
                let old_label = levels.get((code - 1) as usize);
                old_label
                    .and_then(|label| new_positions.get(label))
                    .copied()
                    .unwrap_or(NA_INTEGER)
            };
        }
        set_factor_attrs(result, &new_levels);
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        result
    }
}

/// R's `droplevels(x)` — remove unused levels from a factor.
pub unsafe fn do_droplevels(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        let Some(levels) = aggregate_factor_levels(x) else {
            return x;
        };
        let mut used = BTreeSet::new();
        for i in 0..XLENGTH(x) {
            let code = *INTEGER(x).add(i as usize);
            if code != NA_INTEGER && code > 0 {
                let old_index = (code - 1) as usize;
                if old_index < levels.len() {
                    used.insert(old_index);
                }
            }
        }
        let mut new_levels = Vec::new();
        let mut old_to_new = BTreeMap::new();
        for (old_index, level) in levels.iter().enumerate() {
            if used.contains(&old_index) {
                new_levels.push(level.clone());
                old_to_new.insert(old_index, new_levels.len() as i32);
            }
        }

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, XLENGTH(x));
        if result.is_null() {
            return result;
        }
        let _result_guard = protect(result);
        for i in 0..XLENGTH(x) {
            let code = *INTEGER(x).add(i as usize);
            *INTEGER(result).add(i as usize) = if code == NA_INTEGER || code <= 0 {
                NA_INTEGER
            } else {
                old_to_new
                    .get(&((code - 1) as usize))
                    .copied()
                    .unwrap_or(NA_INTEGER)
            };
        }
        set_factor_attrs(result, &new_levels);
        if inherits_class(x, "ordered") {
            set_ordered_factor_class(result);
        }
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        result
    }
}

/// R's `factor(x)` — create a minimal factor with sorted levels.
pub unsafe fn do_factor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t != SEXPTYPE::STRSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::REALSXP {
            return x;
        }

        let levels_arg = arg_by_name_or_position(args, &["levels"], 1);
        let levels = if levels_arg.is_null() || levels_arg == R_NilValue() {
            let mut level_set = std::collections::BTreeSet::new();
            for i in 0..n {
                if !factor_element_is_na(x, i) {
                    level_set.insert(elt_to_string(x, i));
                }
            }
            level_set.into_iter().collect::<Vec<_>>()
        } else {
            explicit_factor_levels(levels_arg)
        };

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let dst = INTEGER(result);
        for i in 0..n {
            if factor_element_is_na(x, i) {
                *dst.add(i as usize) = NA_INTEGER;
                continue;
            }
            let value = elt_to_string(x, i);
            let code = levels
                .iter()
                .position(|level| level == &value)
                .map(|idx| idx as i32 + 1)
                .unwrap_or(NA_INTEGER);
            *dst.add(i as usize) = code;
        }

        let levels_vec = Rf_allocVector3(SEXPTYPE::STRSXP, levels.len() as R_xlen_t);
        let _levels_vec_guard = protect(levels_vec);
        for (i, level) in levels.iter().enumerate() {
            let cstr = CString::new(level.as_str()).unwrap_or_default();
            SET_STRING_ELT(levels_vec, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }

        let class = Rf_mkString(c"factor".as_ptr());
        let _class_guard = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_LevelsSymbol(),
            levels_vec,
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// R's `ordered(x, levels = ...)` — construct an ordered factor.
pub unsafe fn do_ordered(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let result = do_factor(call, op, args, rho);
        if !result.is_null() && result != R_NilValue() && TYPEOF(result) == SEXPTYPE::INTSXP {
            set_ordered_factor_class(result);
        }
        result
    }
}

/// R's `as.factor(x)` — factors are returned unchanged; other atomic values use factor().
pub unsafe fn do_as_factor(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if aggregate_factor_levels(x).is_some() {
            x
        } else {
            do_factor(call, op, args, rho)
        }
    }
}

/// R's `as.ordered(x)` — coerce to an ordered factor.
pub unsafe fn do_as_ordered(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if inherits_class(x, "ordered") {
            return x;
        }
        if let Some(levels) = aggregate_factor_levels(x) {
            return ordered_from_factor(x, &levels);
        }
        do_ordered(call, op, args, rho)
    }
}

unsafe fn ordered_from_factor(x: SEXP, levels: &[String]) -> SEXP {
    unsafe {
        let mut used = BTreeSet::new();
        for i in 0..XLENGTH(x) {
            let code = *INTEGER(x).add(i as usize);
            if code != NA_INTEGER && code > 0 {
                let old_index = (code - 1) as usize;
                if old_index < levels.len() {
                    used.insert(old_index);
                }
            }
        }
        let mut new_levels = Vec::new();
        let mut old_to_new = BTreeMap::new();
        for (old_index, level) in levels.iter().enumerate() {
            if used.contains(&old_index) {
                new_levels.push(level.clone());
                old_to_new.insert(old_index, new_levels.len() as i32);
            }
        }

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, XLENGTH(x));
        if result.is_null() {
            return result;
        }
        let _result_guard = protect(result);
        for i in 0..XLENGTH(x) {
            let code = *INTEGER(x).add(i as usize);
            *INTEGER(result).add(i as usize) = if code == NA_INTEGER || code <= 0 {
                NA_INTEGER
            } else {
                old_to_new
                    .get(&((code - 1) as usize))
                    .copied()
                    .unwrap_or(NA_INTEGER)
            };
        }
        set_factor_attrs(result, &new_levels);
        set_ordered_factor_class(result);
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        result
    }
}

/// R's `gl(n, k, length = n*k, labels = seq_len(n), ordered = FALSE)`.
pub unsafe fn do_gl(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = arg_by_name_or_position(args, &["n"], 0);
        let k_arg = arg_by_name_or_position(args, &["k"], 1);
        let n = gl_nonnegative_count(n_arg, "argument must be coercible to non-negative integer");
        let k = gl_nonnegative_count(k_arg, "invalid 'times' value");
        let default_len = n.saturating_mul(k);
        let length_arg = arg_by_name_or_position(args, &["length"], 2);
        let out_len = if length_arg.is_null() || length_arg == R_NilValue() {
            default_len
        } else {
            gl_nonnegative_count(length_arg, "invalid 'length.out' value")
        };

        let labels_arg = arg_by_name_or_position(args, &["labels"], 3);
        let levels = if labels_arg.is_null() || labels_arg == R_NilValue() {
            (1..=n).map(|i| i.to_string()).collect::<Vec<_>>()
        } else {
            (0..XLENGTH(labels_arg))
                .map(|i| elt_to_string(labels_arg, i))
                .collect::<Vec<_>>()
        };
        let ordered_arg = arg_by_name_or_position(args, &["ordered"], 4);
        let ordered = if ordered_arg.is_null() || ordered_arg == R_NilValue() {
            false
        } else {
            match logical_arg_by_name_or_position(args, "ordered", 4) {
                Some(value) => value,
                None => {
                    std::panic::panic_any(RError {
                        message: "missing value where TRUE/FALSE needed".to_string(),
                    });
                }
            }
        };

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, out_len);
        if result.is_null() {
            return result;
        }
        let _result_guard = protect(result);
        for i in 0..out_len {
            let code = if n == 0 || k == 0 {
                NA_INTEGER
            } else {
                ((i / k) % n + 1) as i32
            };
            *INTEGER(result).add(i as usize) = code;
        }
        set_factor_attrs(result, &levels);
        if ordered {
            set_ordered_factor_class(result);
        }
        result
    }
}

unsafe fn gl_nonnegative_count(arg: SEXP, error_message: &str) -> R_xlen_t {
    unsafe {
        let value = if arg.is_null() || arg == R_NilValue() || XLENGTH(arg) == 0 {
            NA_REAL
        } else if TYPEOF(arg) == SEXPTYPE::REALSXP {
            *REAL(arg)
        } else if TYPEOF(arg) == SEXPTYPE::INTSXP || TYPEOF(arg) == SEXPTYPE::LGLSXP {
            let raw = *INTEGER(arg);
            if raw == NA_INTEGER {
                NA_REAL
            } else {
                raw as f64
            }
        } else {
            NA_REAL
        };
        if ISNAN(value) || !value.is_finite() || value < 0.0 {
            std::panic::panic_any(RError {
                message: error_message.to_string(),
            });
        }
        value.floor() as R_xlen_t
    }
}

unsafe fn set_ordered_factor_class(x: SEXP) {
    unsafe {
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if class.is_null() {
            return;
        }
        let _class_guard = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"ordered".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"factor".as_ptr()));
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_ClassSymbol(), class);
    }
}

/// R's `addNA(x, ifany = FALSE)` — turn missing values into an explicit factor level.
pub unsafe fn do_addNA(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let ifany = named_logical_arg(args, "ifany").unwrap_or(false);
        let n = XLENGTH(x);
        let is_factor = aggregate_factor_levels(x).is_some();
        let mut levels = if let Some(levels) = aggregate_factor_levels(x) {
            levels.into_iter().map(Some).collect::<Vec<_>>()
        } else {
            let mut level_set = BTreeSet::new();
            for i in 0..n {
                if !factor_element_is_na(x, i) {
                    level_set.insert(elt_to_string(x, i));
                }
            }
            level_set.into_iter().map(Some).collect::<Vec<_>>()
        };
        let has_missing = (0..n).any(|i| factor_element_is_na(x, i));
        let add_missing_level = !ifany || has_missing;
        let missing_code = if add_missing_level {
            levels.push(None);
            levels.len() as i32
        } else {
            NA_INTEGER
        };

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return result;
        }
        let _result_guard = protect(result);
        for i in 0..n {
            *INTEGER(result).add(i as usize) = if factor_element_is_na(x, i) {
                missing_code
            } else if is_factor {
                *INTEGER(x).add(i as usize)
            } else {
                let value = elt_to_string(x, i);
                levels
                    .iter()
                    .position(|level| level.as_deref() == Some(value.as_str()))
                    .map(|idx| idx as i32 + 1)
                    .unwrap_or(NA_INTEGER)
            };
        }
        set_factor_attrs_with_optional_levels(result, &levels);
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        result
    }
}

unsafe fn set_factor_attrs_with_optional_levels(column: SEXP, levels: &[Option<String>]) {
    unsafe {
        let levels_sexp = Rf_allocVector3(SEXPTYPE::STRSXP, levels.len() as R_xlen_t);
        if levels_sexp.is_null() {
            return;
        }
        let _levels_guard = protect(levels_sexp);
        for (i, level) in levels.iter().enumerate() {
            let charsxp = if let Some(level) = level {
                let level_c = CString::new(level.as_str()).unwrap_or_default();
                Rf_mkChar(level_c.as_ptr())
            } else {
                crate::sexp::globals::R_NaString()
            };
            SET_STRING_ELT(levels_sexp, i as R_xlen_t, charsxp);
        }
        crate::sexp::attrib_core::setAttrib(
            column,
            crate::sexp::attrib_core::R_LevelsSymbol(),
            levels_sexp,
        );

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if class.is_null() {
            return;
        }
        let _class_guard = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"factor".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            column,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
    }
}

fn factor_element_is_na(x: SEXP, i: R_xlen_t) -> bool {
    unsafe {
        let ty = TYPEOF(x);
        if ty == SEXPTYPE::STRSXP {
            is_string_na(x, i)
        } else if ty == SEXPTYPE::INTSXP {
            let n = XLENGTH(x);
            n != 0 && *INTEGER(x).add((i % n) as usize) == NA_INTEGER
        } else if ty == SEXPTYPE::REALSXP {
            let n = XLENGTH(x);
            if n == 0 {
                false
            } else {
                let value = *REAL(x).add((i % n) as usize);
                value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || value.is_nan()
            }
        } else {
            false
        }
    }
}

fn explicit_factor_levels(levels_arg: SEXP) -> Vec<String> {
    unsafe {
        let mut levels = Vec::new();
        for i in 0..XLENGTH(levels_arg) {
            if factor_element_is_na(levels_arg, i) {
                continue;
            }
            let level = elt_to_string(levels_arg, i);
            if !levels.iter().any(|existing| existing == &level) {
                levels.push(level);
            }
        }
        levels
    }
}

/// R's `is.factor(x)` — check if factor (simplified: checks class attribute).
pub unsafe fn do_is_factor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        // Check class attribute for "factor"
        let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            let n = XLENGTH(class);
            for i in 0..n {
                let charsxp = STRING_ELT(class, i);
                if !charsxp.is_null() {
                    let s = CHAR(charsxp);
                    if !s.is_null() {
                        let cls = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                        if cls == "factor" {
                            return Rf_ScalarLogical(TRUE);
                        }
                    }
                }
            }
        }
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `is.ordered(x)` — check if ordered factor (simplified).
pub unsafe fn do_is_ordered(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            let n = XLENGTH(class);
            for i in 0..n {
                let charsxp = STRING_ELT(class, i);
                if !charsxp.is_null() {
                    let s = CHAR(charsxp);
                    if !s.is_null() {
                        let cls = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                        if cls == "ordered" {
                            return Rf_ScalarLogical(TRUE);
                        }
                    }
                }
            }
        }
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `levels(x)` — factor levels (simplified).
pub unsafe fn do_levels(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Get levels attribute
        let levels = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"levels".as_ptr()));
        if levels.is_null() {
            return R_NilValue();
        }
        levels
    }
}

/// R's `levels(x) <- value` — replace factor levels or the raw levels attribute.
pub unsafe fn do_levels_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let result = if inherits_class(x, "factor") {
            replace_factor_levels(x, value)
        } else {
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_LevelsSymbol(),
                value,
            );
            x
        };

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        result
    }
}

unsafe fn replace_factor_levels(x: SEXP, value: SEXP) -> SEXP {
    unsafe {
        let old_levels = string_vector_values(crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_LevelsSymbol(),
        ));
        let xlevs = if TYPEOF(value) == SEXPTYPE::VECSXP {
            factor_levels_from_named_list(value, &old_levels)
        } else {
            if XLENGTH(value) < old_levels.len() as R_xlen_t {
                std::panic::panic_any(RError {
                    message: "number of levels differs".to_string(),
                });
            }
            (0..XLENGTH(value))
                .map(|i| {
                    if is_string_na(value, i) {
                        None
                    } else {
                        Some(elt_to_string(value, i))
                    }
                })
                .collect::<Vec<_>>()
        };

        let new_levels = unique_present_strings(&xlevs);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, XLENGTH(x));
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..XLENGTH(x) {
            let old_code = if TYPEOF(x) == SEXPTYPE::INTSXP {
                INTEGER_ELT(x, i as c_int)
            } else {
                NA_INTEGER
            };
            let new_code = if old_code == NA_INTEGER || old_code <= 0 {
                NA_INTEGER
            } else {
                let old_idx = (old_code - 1) as usize;
                xlevs
                    .get(old_idx)
                    .and_then(|level| level.as_ref())
                    .and_then(|level| match_string(&new_levels, level))
                    .map(|idx| idx as c_int + 1)
                    .unwrap_or(NA_INTEGER)
            };
            *INTEGER(result).add(i as usize) = new_code;
        }

        crate::sexp::accessors::SET_ATTRIB(result, ATTRIB(x));
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_LevelsSymbol(),
            string_vector(&new_levels),
        );
        result
    }
}

unsafe fn factor_levels_from_named_list(value: SEXP, old_levels: &[String]) -> Vec<Option<String>> {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(value, crate::sexp::attrib_core::R_NamesSymbol());
        let mut xlevs = old_levels.iter().cloned().map(Some).collect::<Vec<_>>();
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return xlevs;
        }

        for group_idx in 0..XLENGTH(value) {
            if is_string_na(names, group_idx) {
                continue;
            }
            let group_name = elt_to_string(names, group_idx);
            let members = VECTOR_ELT(value, group_idx);
            for member_idx in 0..XLENGTH(members) {
                let old_name = elt_to_string(members, member_idx);
                if let Some(pos) = match_string(old_levels, &old_name)
                    && let Some(slot) = xlevs.get_mut(pos)
                {
                    *slot = Some(group_name.clone());
                }
            }
        }
        xlevs
    }
}

fn unique_present_strings(values: &[Option<String>]) -> Vec<String> {
    let mut unique = Vec::new();
    for value in values.iter().flatten() {
        if !unique.iter().any(|existing| existing == value) {
            unique.push(value.clone());
        }
    }
    unique
}

fn match_string(values: &[String], needle: &str) -> Option<usize> {
    values.iter().position(|value| value == needle)
}

pub(crate) unsafe fn inherits_class(x: SEXP, class_name: &str) -> bool {
    unsafe {
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return false;
        }
        (0..XLENGTH(class)).any(|i| elt_to_string(class, i) == class_name)
    }
}

/// R's `nlevels(x)` — number of levels (simplified).
pub unsafe fn do_nlevels(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        let levels = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"levels".as_ptr()));
        if levels.is_null() {
            return Rf_ScalarInteger(0);
        }
        Rf_ScalarInteger(XLENGTH(levels) as i32)
    }
}
