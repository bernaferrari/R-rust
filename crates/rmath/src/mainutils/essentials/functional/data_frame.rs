//! data.frame(), list accessors and c().

use super::*;

pub(crate) fn recycle_column_if_needed(x: SEXP, target_len: R_xlen_t) -> SEXP {
    // GNU recycles data.frame columns by *rows*. A 2-D AsIs / model.matrix
    // column has length nrow*ncol, so XLENGTH is the wrong unit.
    let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
    let len = if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2 {
        integer_elt_checked(dim, 0) as R_xlen_t
    } else {
        XLENGTH(x)
    };
    if len == target_len || target_len == 0 {
        return x;
    }
    if len != 1 {
        base_error(format!(
            "arguments imply differing number of rows: {target_len}, {len}"
        ));
    }
    let ty = TYPEOF(x);
    let out = Rf_allocVector3(ty, target_len);
    if out.is_null() {
        return out;
    }
    let _out_guard = protect(out);
    for i in 0..target_len {
        match ty {
            t if t == SEXPTYPE::REALSXP => set_real_checked(out, i, real_elt_checked(x, 0)),
            t if t == SEXPTYPE::INTSXP => set_integer_checked(out, i, integer_elt_checked(x, 0)),
            t if t == SEXPTYPE::LGLSXP => set_logical_checked(out, i, logical_elt_checked(x, 0)),
            t if t == SEXPTYPE::STRSXP => SET_STRING_ELT(out, i, STRING_ELT(x, 0)),
            _ => return x,
        }
    }
    // GNU recycling preserves the column's attributes (class — e.g. the
    // AsIs marker on I(x) columns — tsp, units, ...)))); only names/dim/
    // dimnames are dropped, exactly copyMostAttrib semantics.
    copy_most_attrib(x, out);
    out
}

pub(crate) fn melt_table(value: SEXP) -> SEXP {
    let dim = crate::sexp::attrib_core::getAttrib(value, crate::sexp::attrib_core::R_DimSymbol());
    let nd = XLENGTH(dim) as usize;
    let mut sizes = Vec::with_capacity(nd);
    let mut total: i64 = 1;
    for i in 0..nd {
        let n = integer_elt_checked(dim, (i) as R_xlen_t) as i64;
        sizes.push(n);
        total = total.saturating_mul(n.max(0));
    }
    let frame = Rf_allocVector3(SEXPTYPE::VECSXP, (nd as i64) + 1);
    let _frame_guard = protect(frame);
    let dimnames =
        crate::sexp::attrib_core::getAttrib(value, crate::sexp::attrib_core::R_DimNamesSymbol());
    let dimnames_names = if !dimnames.is_null() && dimnames != R_NilValue() {
        crate::sexp::attrib_core::getAttrib(dimnames, crate::sexp::attrib_core::R_NamesSymbol())
    } else {
        R_NilValue()
    };
    let mut names = Vec::with_capacity(nd + 1);
    for d in 0..nd {
        let col = Rf_allocVector3(SEXPTYPE::STRSXP, total);
        SET_VECTOR_ELT(frame, d as i64, col);
        let levels = if !dimnames.is_null()
            && TYPEOF(dimnames) == SEXPTYPE::VECSXP
            && (d as i64) < XLENGTH(dimnames)
        {
            VECTOR_ELT(dimnames, d as i64)
        } else {
            R_NilValue()
        };
        for idx in 0..total {
            let mut rem = idx;
            for s in &sizes[..d] {
                rem /= (*s).max(1);
            }
            let level = if sizes[d] == 0 { 0 } else { rem % sizes[d] };
            let text = if !levels.is_null()
                && TYPEOF(levels) == SEXPTYPE::STRSXP
                && level < XLENGTH(levels)
            {
                elt_to_string(levels, level)
            } else {
                (level + 1).to_string()
            };
            let cstr = std::ffi::CString::new(text).unwrap_or_default();
            SET_STRING_ELT(col, idx, mk_char_cstr(&cstr));
        }
        let nm = if !dimnames_names.is_null()
            && TYPEOF(dimnames_names) == SEXPTYPE::STRSXP
            && (d as i64) < XLENGTH(dimnames_names)
        {
            elt_to_string(dimnames_names, d as i64)
        } else {
            String::new()
        };
        names.push(if nm.is_empty() {
            format!("Var{}", d + 1)
        } else {
            nm
        });
    }
    let freq = Rf_allocVector3(SEXPTYPE::INTSXP, total);
    SET_VECTOR_ELT(frame, nd as i64, freq);
    for idx in 0..total {
        let v = if TYPEOF(value) == SEXPTYPE::INTSXP {
            integer_elt_checked(value, (idx as usize) as R_xlen_t)
        } else if TYPEOF(value) == SEXPTYPE::REALSXP {
            real_elt_checked(value, (idx as usize) as R_xlen_t) as i32
        } else {
            0
        };
        set_integer_checked(freq, (idx as usize) as R_xlen_t, v);
    }
    names.push("Freq".to_string());
    set_string_names(frame, &names);
    set_compact_row_names(frame, total);
    set_data_frame_class(frame);
    frame
}

/// R's `data.frame(...)`: build a data-frame list while expanding data-frame arguments.
pub fn do_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // `...` are the columns; the remaining formals (row.names,
    // check.rows, check.names, fix.empty.names, stringsAsFactors) are
    // options, never columns. Drop option cells before list-building so
    // `data.frame(a=1, stringsAsFactors=FALSE)` keeps 1 column.
    let mut filtered = R_NilValue();
    let mut tail: SEXP = std::ptr::null_mut();
    let mut filter_guards: Vec<_> = Vec::new();
    let mut ap = args;
    while !ap.is_null() && ap != R_NilValue() {
        let is_option = matches!(
            tag_name(ap).as_deref(),
            Some("row.names")
                | Some("check.rows")
                | Some("check.names")
                | Some("fix.empty.names")
                | Some("stringsAsFactors")
        );
        if !is_option {
            let cell = Rf_cons(CAR(ap), R_NilValue());
            filter_guards.push(protect(cell));
            let tg = TAG(ap);
            if !tg.is_null() && tg != R_NilValue() {
                SETTAG(cell, tg);
            }
            if filtered == R_NilValue() {
                filtered = cell;
            } else {
                SETCDR(tail, cell);
            }
            tail = cell;
        }
        ap = CDR(ap);
    }
    let _filtered_guard = protect(filtered);
    let initial = do_list(_call, _op, filtered, _rho);
    if initial.is_null() || initial == R_NilValue() {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        if !result.is_null() {
            let _result_guard = protect(result);
            set_string_names(result, &[]);
            set_compact_row_names(result, 0);
            set_data_frame_class(result);
        }
        return result;
    }
    let _initial_guard = protect(initial);
    let arg_names =
        crate::sexp::attrib_core::getAttrib(initial, crate::sexp::attrib_core::R_NamesSymbol());
    let mut columns: Vec<SEXP> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut nrow: Option<R_xlen_t> = None;

    for i in 0..XLENGTH(initial) {
        let mut value = VECTOR_ELT(initial, i);
        if sexp_has_class(value, "table") {
            value = melt_table(value);
            filter_guards.push(protect(value));
        }
        let arg_name = string_at_or_empty(arg_names, i);
        if sexp_has_class(value, "data.frame") && TYPEOF(value) == SEXPTYPE::VECSXP {
            let inner_names = crate::sexp::attrib_core::getAttrib(
                value,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            for j in 0..XLENGTH(value) {
                let column = VECTOR_ELT(value, j);
                let len = XLENGTH(column);
                match nrow {
                    Some(existing) if len != existing => base_error(format!(
                        "arguments imply differing number of rows: {existing}, {len}"
                    )),
                    None => nrow = Some(len),
                    _ => {}
                }
                columns.push(column);
                let child_name = string_at_or_empty(inner_names, j);
                names.push(if arg_name.is_empty() {
                    child_name
                } else if child_name.is_empty() {
                    arg_name.clone()
                } else {
                    format!("{arg_name}.{child_name}")
                });
            }
        } else if TYPEOF(crate::sexp::attrib_core::getAttrib(
            value,
            crate::sexp::attrib_core::R_DimSymbol(),
        )) == SEXPTYPE::INTSXP
            && XLENGTH(crate::sexp::attrib_core::getAttrib(
                value,
                crate::sexp::attrib_core::R_DimSymbol(),
            )) == 2
        {
            let dim =
                crate::sexp::attrib_core::getAttrib(value, crate::sexp::attrib_core::R_DimSymbol());
            let nr = integer_elt_checked(dim, 0) as R_xlen_t;
            let nc = integer_elt_checked(dim, (1) as R_xlen_t) as R_xlen_t;
            match nrow {
                Some(existing) if nr != existing && nr != 0 && existing != 0 => base_error(
                    format!("arguments imply differing number of rows: {existing}, {nr}"),
                ),
                None => nrow = Some(nr),
                _ => {}
            }
            if nr != 0 || nc == 0 {
                nrow = Some(nrow.unwrap_or(nr));
            }
            // GNU `as.data.frame.AsIs` (and `as.data.frame.model.matrix`):
            // a 2-D AsIs object stays one list column. Bare matrices split.
            if sexp_has_class(value, "AsIs") || sexp_has_class(value, "model.matrix") {
                columns.push(value);
                names.push(arg_name);
            } else {
                for j in 0..nc {
                    columns.push(super::s3::matrix_column(value, nr, j));
                    names.push(if arg_name.is_empty() {
                        format!("V{}", j + 1)
                    } else if nc == 1 {
                        arg_name.clone()
                    } else {
                        format!("{arg_name}.{}", j + 1)
                    });
                }
            }
        } else {
            let value = if sexp_has_class(value, "POSIXlt") && TYPEOF(value) == SEXPTYPE::VECSXP {
                let ct = crate::mainutils::essentials::do_as_POSIXct(
                    _call,
                    _op,
                    Rf_cons(value, R_NilValue()),
                    _rho,
                );
                filter_guards.push(protect(ct));
                ct
            } else {
                value
            };
            let len = XLENGTH(value);
            match nrow {
                Some(existing) if len != existing && len != 1 => base_error(format!(
                    "arguments imply differing number of rows: {existing}, {len}"
                )),
                None => nrow = Some(len),
                _ => {}
            }
            columns.push(value);
            names.push(arg_name);
        }
    }

    repair_data_frame_names(&mut names);
    let row_count = nrow.unwrap_or(0);
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, columns.len() as R_xlen_t);
    if result.is_null() {
        return result;
    }
    let _result_guard = protect(result);
    for (i, column) in columns.iter().enumerate() {
        SET_VECTOR_ELT(
            result,
            i as R_xlen_t,
            recycle_column_if_needed(*column, row_count),
        );
    }
    set_string_names(result, &names);
    set_compact_row_names(result, row_count);
    set_data_frame_class(result);

    result
}

// ---------------------------------------------------------------------------
// List operations
// ---------------------------------------------------------------------------





