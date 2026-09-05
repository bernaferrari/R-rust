//! Binding and statistics runtime: cbind/rbind fast paths, var, sd, median, IQR, cummin/cummax — extracted verbatim from the former single-file module.
use super::*;

// ---------------------------------------------------------------------------
// Complete R runtime — cbind, rbind, t (transpose), and other critical functions
// ---------------------------------------------------------------------------

/// R's `cbind(...)` — combine vectors/matrices by columns.
pub unsafe fn do_cbind(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut result_type = SEXPTYPE::LGLSXP;
        let mut col_names = Vec::new();
        let mut has_col_names = false;
        let mut entries = Vec::new();

        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                result_type = bind_common_type(result_type, SEXPTYPE(TYPEOF(arg)));
                let (arg_nrow, arg_ncol) = bind_dims(arg, true);
                let name = tag_name(current).unwrap_or_default();
                if !name.is_empty() {
                    has_col_names = true;
                }
                entries.push((arg, arg_nrow, arg_ncol, name));
            }
            current = CDR(current);
        }

        let has_nonzero_extent = entries
            .iter()
            .any(|&(_, nrow, ncol, _)| nrow > 0 && ncol > 0);
        let mut ncols: R_xlen_t = 0;
        let mut nrows: R_xlen_t = 0;
        for &(_, arg_nrow, arg_ncol, ref name) in &entries {
            if has_nonzero_extent && (arg_nrow == 0 || arg_ncol == 0) {
                continue;
            }
            nrows = nrows.max(arg_nrow);
            ncols += arg_ncol;
            for j in 0..arg_ncol {
                if arg_ncol == 1 {
                    col_names.push(name.clone());
                } else if name.is_empty() {
                    col_names.push(String::new());
                } else {
                    col_names.push(format!("{name}.{j_plus}", j_plus = j + 1));
                }
            }
        }

        if nrows == 0 || ncols == 0 {
            let result = Rf_allocVector3(result_type, 0);
            if result.is_null() {
                return R_NilValue();
            }
            set_two_dim_attr(result, nrows, ncols);
            if has_col_names {
                set_bind_dimnames(result, R_NilValue(), string_vector(&col_names));
            }
            return result;
        }

        let total = nrows * ncols;
        let result = Rf_allocVector3(result_type, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let mut col_offset: R_xlen_t = 0;
        for &(arg, arg_nrow, arg_ncol, _) in &entries {
            if has_nonzero_extent && (arg_nrow == 0 || arg_ncol == 0) {
                continue;
            }
            let arg_len = XLENGTH(arg);
            if arg_len == 0 {
                continue;
            }

            for j in 0..arg_ncol {
                for i in 0..nrows {
                    let src_idx = ((j * arg_nrow + (i % arg_nrow)) % arg_len) as R_xlen_t;
                    let dst_idx = ((col_offset + j) * nrows + i) as R_xlen_t;
                    copy_bind_value(result, dst_idx, result_type, arg, src_idx);
                }
            }
            col_offset += arg_ncol;
        }

        set_two_dim_attr(result, nrows, ncols);
        if has_col_names {
            set_bind_dimnames(result, R_NilValue(), string_vector(&col_names));
        }
        result
    }
}

/// R's `rbind(...)` — combine vectors/matrices by rows.
pub unsafe fn do_rbind(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // GNU R dispatches `rbind` through S3.  This runtime registers a
        // direct builtin, so retain the key dispatch boundary explicitly:
        // a data-frame first argument must be bound column-by-column rather
        // than flattened as the VECSXP storage of an atomic matrix.
        // GNU R's rbind finds the dispatching object after skipping NULL
        // arguments, so `rbind(NULL, df)` binds column-by-column too —
        // read.fortunes() seeds its accumulation with exactly that call.
        let mut dispatch = args;
        while !dispatch.is_null()
            && dispatch != R_NilValue()
            && (CAR(dispatch).is_null() || CAR(dispatch) == R_NilValue())
        {
            dispatch = CDR(dispatch);
        }
        let first = if dispatch.is_null() || dispatch == R_NilValue() {
            R_NilValue()
        } else {
            CAR(dispatch)
        };
        if sexp_has_class(first, "data.frame") && TYPEOF(first) == SEXPTYPE::VECSXP {
            return rbind_data_frame(dispatch);
        }

        let mut result_type = SEXPTYPE::LGLSXP;
        let mut row_names = Vec::new();
        let mut has_row_names = false;
        let mut entries = Vec::new();

        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                result_type = bind_common_type(result_type, SEXPTYPE(TYPEOF(arg)));
                let (arg_nrow, arg_ncol) = bind_dims(arg, false);
                let name = tag_name(current).unwrap_or_default();
                if !name.is_empty() {
                    has_row_names = true;
                }
                entries.push((arg, arg_nrow, arg_ncol, name));
            }
            current = CDR(current);
        }

        let has_nonzero_extent = entries
            .iter()
            .any(|&(_, nrow, ncol, _)| nrow > 0 && ncol > 0);
        let mut ncols: R_xlen_t = 0;
        let mut nrows: R_xlen_t = 0;
        for &(_, arg_nrow, arg_ncol, ref name) in &entries {
            if has_nonzero_extent && (arg_nrow == 0 || arg_ncol == 0) {
                continue;
            }
            ncols = ncols.max(arg_ncol);
            nrows += arg_nrow;
            for i in 0..arg_nrow {
                if arg_nrow == 1 {
                    row_names.push(name.clone());
                } else if name.is_empty() {
                    row_names.push(String::new());
                } else {
                    row_names.push(format!("{name}.{i_plus}", i_plus = i + 1));
                }
            }
        }

        if nrows == 0 || ncols == 0 {
            let result = Rf_allocVector3(result_type, 0);
            if result.is_null() {
                return R_NilValue();
            }
            set_two_dim_attr(result, nrows, ncols);
            if has_row_names {
                set_bind_dimnames(result, string_vector(&row_names), R_NilValue());
            }
            return result;
        }

        let total = nrows * ncols;
        let result = Rf_allocVector3(result_type, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let mut row_offset: R_xlen_t = 0;
        for &(arg, arg_nrow, arg_ncol, _) in &entries {
            if has_nonzero_extent && (arg_nrow == 0 || arg_ncol == 0) {
                continue;
            }
            let arg_len = XLENGTH(arg);
            if arg_len == 0 {
                continue;
            }

            for j in 0..ncols {
                for i in 0..arg_nrow {
                    let src_idx = ((j * arg_nrow + i) % arg_len) as R_xlen_t;
                    let dst_idx = (j * nrows + row_offset + i) as R_xlen_t;
                    copy_bind_value(result, dst_idx, result_type, arg, src_idx);
                }
            }
            row_offset += arg_nrow;
        }

        set_two_dim_attr(result, nrows, ncols);
        if has_row_names {
            set_bind_dimnames(result, string_vector(&row_names), R_NilValue());
        }
        result
    }
}

/// Core `rbind.data.frame` behavior for atomic columns and atomic/list rows.
///
/// Each output column computes its own common type. Data-frame arguments are
/// matched to the first frame by column name; vector/list arguments add one
/// row and are matched by name when every element is named.
unsafe fn rbind_data_frame(args: SEXP) -> SEXP {
    unsafe {
        let template = CAR(args);
        let ncols = XLENGTH(template);
        let template_names = crate::sexp::attrib_core::getAttrib(
            template,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let column_names: Vec<String> = (0..ncols)
            .map(|i| string_at_or_empty(template_names, i))
            .collect();

        // A source vector and index for every result cell, grouped by column.
        let mut cells: Vec<Vec<(SEXP, R_xlen_t)>> = vec![Vec::new(); ncols as usize];
        let mut result_row_names = Vec::new();
        let mut next_automatic_row = 1usize;
        let mut current = args;

        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            if value.is_null() || value == R_NilValue() {
                current = CDR(current);
                continue;
            }

            if sexp_has_class(value, "data.frame") && TYPEOF(value) == SEXPTYPE::VECSXP {
                if XLENGTH(value) != ncols {
                    base_error("numbers of columns of arguments do not match".to_string());
                }
                let value_names = crate::sexp::attrib_core::getAttrib(
                    value,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                );
                let source_columns: Vec<R_xlen_t> = column_names
                    .iter()
                    .enumerate()
                    .map(|(fallback, wanted)| {
                        (0..ncols)
                            .find(|&j| string_at_or_empty(value_names, j) == *wanted)
                            .unwrap_or(fallback as R_xlen_t)
                    })
                    .collect();
                let nrows = data_frame_row_count(value);
                let source_row_names = if nrows == 0 {
                    Vec::new()
                } else {
                    data_frame_row_names(value)
                };
                for (out_col, &source_col) in source_columns.iter().enumerate() {
                    let column = VECTOR_ELT(value, source_col);
                    if XLENGTH(column) != nrows {
                        base_error("invalid data frame column length".to_string());
                    }
                    for row in 0..nrows {
                        cells[out_col].push((column, row));
                    }
                }
                for row_name in source_row_names {
                    result_row_names.push(row_name);
                    next_automatic_row += 1;
                }
            } else {
                if XLENGTH(value) != ncols {
                    base_error(format!(
                        "number of columns of result, {ncols}, is not a multiple of vector length {} of arg",
                        XLENGTH(value)
                    ));
                }
                let value_names = crate::sexp::attrib_core::getAttrib(
                    value,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                );
                let fully_named = TYPEOF(value_names) == SEXPTYPE::STRSXP
                    && XLENGTH(value_names) == ncols
                    && (0..ncols).all(|i| !string_at_or_empty(value_names, i).is_empty());
                for (out_col, wanted) in column_names.iter().enumerate() {
                    let source_col = if fully_named {
                        (0..ncols)
                            .find(|&j| string_at_or_empty(value_names, j) == *wanted)
                            .unwrap_or(out_col as R_xlen_t)
                    } else {
                        out_col as R_xlen_t
                    };
                    cells[out_col].push((value, source_col));
                }
                let tag = tag_name(current).unwrap_or_default();
                result_row_names.push(if tag.is_empty() {
                    next_automatic_row.to_string()
                } else {
                    tag
                });
                next_automatic_row += 1;
            }
            current = CDR(current);
        }

        let nrows = result_row_names.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (column_index, source_cells) in cells.iter().enumerate() {
            let mut result_type = SEXPTYPE::LGLSXP;
            for &(source, source_index) in source_cells {
                let source_type = if TYPEOF(source) == SEXPTYPE::VECSXP {
                    TYPEOF(VECTOR_ELT(source, source_index))
                } else {
                    TYPEOF(source)
                };
                result_type = bind_common_type(result_type, SEXPTYPE(source_type));
            }
            let column = Rf_allocVector3(result_type, nrows);
            if column.is_null() {
                return R_NilValue();
            }
            let column_guard = protect(column);
            for (row, &(source, source_index)) in source_cells.iter().enumerate() {
                if TYPEOF(source) == SEXPTYPE::VECSXP {
                    copy_bind_value(
                        column,
                        row as R_xlen_t,
                        result_type,
                        VECTOR_ELT(source, source_index),
                        0,
                    );
                } else {
                    copy_bind_value(column, row as R_xlen_t, result_type, source, source_index);
                }
            }
            SET_VECTOR_ELT(result, column_index as R_xlen_t, column);
            drop(column_guard);
        }

        set_string_names(result, &column_names);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_RowNamesSymbol(),
            string_vector(&result_row_names),
        );
        set_data_frame_class(result);
        result
    }
}

pub fn bind_common_type(left: SEXPTYPE, right: SEXPTYPE) -> SEXPTYPE {
    if left == SEXPTYPE::STRSXP || right == SEXPTYPE::STRSXP {
        SEXPTYPE::STRSXP
    } else if left == SEXPTYPE::REALSXP || right == SEXPTYPE::REALSXP {
        SEXPTYPE::REALSXP
    } else if left == SEXPTYPE::INTSXP || right == SEXPTYPE::INTSXP {
        SEXPTYPE::INTSXP
    } else {
        left
    }
}

pub unsafe fn bind_dims(arg: SEXP, cbind: bool) -> (R_xlen_t, R_xlen_t) {
    unsafe {
        let dim_attr =
            crate::sexp::attrib_core::getAttrib(arg, crate::sexp::attrib_core::R_DimSymbol());
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            (
                *INTEGER(dim_attr) as R_xlen_t,
                *INTEGER(dim_attr).add(1) as R_xlen_t,
            )
        } else if cbind {
            (XLENGTH(arg), 1)
        } else {
            (1, XLENGTH(arg))
        }
    }
}

pub unsafe fn copy_bind_value(
    dst: SEXP,
    dst_i: R_xlen_t,
    dst_type: SEXPTYPE,
    src: SEXP,
    src_i: R_xlen_t,
) {
    unsafe {
        match dst_type {
            SEXPTYPE::STRSXP => {
                if TYPEOF(src) == SEXPTYPE::STRSXP
                    && STRING_ELT(src, src_i) == crate::sexp::globals::R_NaString()
                {
                    SET_STRING_ELT(dst, dst_i, crate::sexp::globals::R_NaString());
                } else {
                    let value = elt_to_string(src, src_i);
                    let c_value = CString::new(value).unwrap_or_default();
                    SET_STRING_ELT(dst, dst_i, Rf_mkChar(c_value.as_ptr()));
                }
            }
            SEXPTYPE::REALSXP => {
                let value = match SEXPTYPE(TYPEOF(src)) {
                    SEXPTYPE::REALSXP => REAL_ELT(src, src_i as c_int),
                    SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => {
                        let value = INTEGER_ELT(src, src_i as c_int);
                        if value == NA_INTEGER {
                            NA_REAL
                        } else {
                            value as f64
                        }
                    }
                    _ => NA_REAL,
                };
                *REAL(dst).add(dst_i as usize) = value;
            }
            SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => {
                let value = match SEXPTYPE(TYPEOF(src)) {
                    SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => INTEGER_ELT(src, src_i as c_int),
                    _ => NA_INTEGER,
                };
                *INTEGER(dst).add(dst_i as usize) = value;
            }
            _ => {}
        }
    }
}

pub unsafe fn set_bind_dimnames(result: SEXP, row_names: SEXP, col_names: SEXP) {
    unsafe {
        let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if dimnames.is_null() {
            return;
        }
        let _dimnames_guard = protect(dimnames);
        SET_VECTOR_ELT(dimnames, 0, row_names);
        SET_VECTOR_ELT(dimnames, 1, col_names);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dimnames,
        );
    }
}
/// R's `var(x, y = NULL, na.rm = FALSE)` — variance or covariance.
pub unsafe fn do_var(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let y = CAR(CDR(args));
        let na_rm_arg = CAR(CDR(CDR(args)));
        let na_rm = !na_rm_arg.is_null()
            && na_rm_arg != R_NilValue()
            && real_or_default(na_rm_arg, 0.0) != 0.0;

        if y.is_null() || y == R_NilValue() {
            // Variance of x
            let n = XLENGTH(x);
            let t = TYPEOF(x);
            let mut sum = 0.0f64;
            let mut sum_sq = 0.0f64;
            let mut count = 0i64;

            for i in 0..n {
                let val = if t == SEXPTYPE::REALSXP {
                    *REAL(x).add(i as usize)
                } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };

                if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
                    if !na_rm {
                        return Rf_ScalarReal(NA_REAL);
                    }
                } else {
                    sum += val;
                    sum_sq += val * val;
                    count += 1;
                }
            }

            if count < 2 {
                return Rf_ScalarReal(NA_REAL);
            }

            let mean = sum / count as f64;
            let variance = (sum_sq - count as f64 * mean * mean) / (count - 1) as f64;
            Rf_ScalarReal(variance)
        } else {
            // Covariance of x and y
            let n = XLENGTH(x).min(XLENGTH(y));
            let tx = TYPEOF(x);
            let ty = TYPEOF(y);
            let mut sum_x = 0.0f64;
            let mut sum_y = 0.0f64;
            let mut sum_xy = 0.0f64;
            let mut count = 0i64;

            for i in 0..n {
                let val_x = if tx == SEXPTYPE::REALSXP {
                    *REAL(x).add(i as usize)
                } else if tx == SEXPTYPE::INTSXP || tx == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };

                let val_y = if ty == SEXPTYPE::REALSXP {
                    *REAL(y).add(i as usize)
                } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(y).add(i as usize);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };

                if val_x.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                    || val_y.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                    || val_x.is_nan()
                    || val_y.is_nan()
                {
                    if !na_rm {
                        return Rf_ScalarReal(NA_REAL);
                    }
                } else {
                    sum_x += val_x;
                    sum_y += val_y;
                    sum_xy += val_x * val_y;
                    count += 1;
                }
            }

            if count < 2 {
                return Rf_ScalarReal(NA_REAL);
            }

            let mean_x = sum_x / count as f64;
            let mean_y = sum_y / count as f64;
            let covariance = (sum_xy - count as f64 * mean_x * mean_y) / (count - 1) as f64;
            Rf_ScalarReal(covariance)
        }
    }
}

/// R's `sd(x, na.rm = FALSE)` — standard deviation.
pub unsafe fn do_sd(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Call do_var to get variance
        let var_args = Rf_cons(CAR(args), CDR(args));
        let var_result = do_var(_call, _op, var_args, _rho);
        if var_result.is_null() {
            return R_NilValue();
        }

        let v = real_or_default(var_result, NA_REAL);
        if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v < 0.0 {
            Rf_ScalarReal(NA_REAL)
        } else {
            Rf_ScalarReal(libm::sqrt(v))
        }
    }
}

/// R's `median(x, na.rm = FALSE)` — median value.
pub unsafe fn do_median(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(NA_REAL);
        }

        let na_rm_arg = CAR(CDR(args));
        let na_rm = !na_rm_arg.is_null()
            && na_rm_arg != R_NilValue()
            && real_or_default(na_rm_arg, 0.0) != 0.0;

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let mut vals: Vec<f64> = Vec::new();

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
                if !na_rm {
                    return Rf_ScalarReal(NA_REAL);
                }
            } else {
                vals.push(val);
            }
        }

        if vals.is_empty() {
            return Rf_ScalarReal(NA_REAL);
        }

        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = vals.len() / 2;
        if vals.len().is_multiple_of(2) {
            Rf_ScalarReal((vals[mid - 1] + vals[mid]) / 2.0)
        } else {
            Rf_ScalarReal(vals[mid])
        }
    }
}

/// R's `IQR(x, na.rm = FALSE)` — interquartile range using quantile type 7.
pub unsafe fn do_iqr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(NA_REAL);
        }

        let na_rm_arg = CAR(CDR(args));
        let na_rm = !na_rm_arg.is_null()
            && na_rm_arg != R_NilValue()
            && real_or_default(na_rm_arg, 0.0) != 0.0;

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let mut vals = Vec::new();

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == R_NA_BIT_PATTERN || val.is_nan() {
                if !na_rm {
                    return Rf_ScalarReal(NA_REAL);
                }
            } else {
                vals.push(val);
            }
        }

        if vals.is_empty() {
            return Rf_ScalarReal(NA_REAL);
        }

        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Rf_ScalarReal(iqr_value(&mut vals))
    }
}

/// R's `cummin(x)` — cumulative minimum.
pub unsafe fn do_cummin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result_type = if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            SEXPTYPE::INTSXP
        } else {
            SEXPTYPE::REALSXP
        };
        let result = Rf_allocVector3(result_type, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        if result_type == SEXPTYPE::INTSXP {
            let dst = INTEGER(result);
            let mut min_so_far = i32::MAX;
            let mut poisoned = false;
            for i in 0..n {
                let val = *INTEGER(x).add(i as usize);
                if val == NA_INTEGER {
                    poisoned = true;
                }
                if poisoned {
                    *dst.add(i as usize) = NA_INTEGER;
                } else {
                    min_so_far = min_so_far.min(val);
                    *dst.add(i as usize) = min_so_far;
                }
            }
            return result;
        }

        let dst = REAL(result);

        let mut min_so_far = f64::INFINITY;
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
                min_so_far = NA_REAL;
            } else if min_so_far.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN {
                min_so_far = min_so_far.min(val);
            }
            *dst.add(i as usize) = min_so_far;
        }
        result
    }
}

/// R's `cummax(x)` — cumulative maximum.
pub unsafe fn do_cummax(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result_type = if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            SEXPTYPE::INTSXP
        } else {
            SEXPTYPE::REALSXP
        };
        let result = Rf_allocVector3(result_type, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        if result_type == SEXPTYPE::INTSXP {
            let dst = INTEGER(result);
            let mut max_so_far = i32::MIN;
            let mut poisoned = false;
            for i in 0..n {
                let val = *INTEGER(x).add(i as usize);
                if val == NA_INTEGER {
                    poisoned = true;
                }
                if poisoned {
                    *dst.add(i as usize) = NA_INTEGER;
                } else {
                    max_so_far = max_so_far.max(val);
                    *dst.add(i as usize) = max_so_far;
                }
            }
            return result;
        }

        let dst = REAL(result);

        let mut max_so_far = f64::NEG_INFINITY;
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
                max_so_far = NA_REAL;
            } else if max_so_far.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN {
                max_so_far = max_so_far.max(val);
            }
            *dst.add(i as usize) = max_so_far;
        }
        result
    }
}

#[cfg(test)]
mod rbind_data_frame_tests {
    use crate::sexp::ffi::TRUE;
    use crate::sexp::session::RSession;

    fn assert_r_true(code: &str) {
        let mut session = RSession::new();
        let (result, output, visible) = session.eval_code_with_output_capture(code);
        let result = result.expect("rbind.data.frame expression should evaluate");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(
            output.stdout.is_empty(),
            "unexpected output: {}",
            output.stdout
        );
        assert!(visible);
    }

    #[test]
    fn rbind_preserves_data_frame_shape_and_binds_columns_independently() {
        assert_r_true(
            "d1 <- rbind(data.frame(a=1, b=I(TRUE)), new=c(7, 'N')); \
             is.data.frame(d1) && identical(names(d1), c('a', 'b')) && \
             identical(row.names(d1), c('1', 'new')) && \
             identical(d1$a, c('1', '7')) && identical(d1$b, c('TRUE', 'N')) && \
             is.null(attr(unclass(d1$b), 'class'))",
        );
    }

    #[test]
    fn rbind_matches_data_frame_and_named_row_columns_by_name() {
        assert_r_true(
            "x <- data.frame(a=1:2, b=c('x', 'y')); \
             y <- data.frame(b='z', a=3); \
             z <- rbind(x, y, c(b='w', a='4')); \
             identical(z$a, c('1', '2', '3', '4')) && \
             identical(z$b, c('x', 'y', 'z', 'w'))",
        );
    }
}
