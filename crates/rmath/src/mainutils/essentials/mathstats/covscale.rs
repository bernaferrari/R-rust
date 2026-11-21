//! cov/cor/scale, rle, rowsum, jitter, margin.table, mad/fivenum/boxplot.stats, p.adjust — extracted verbatim from essentials/mathstats.rs.

use super::*;

/// R's `cov(x, y)` — covariance between two numeric vectors.
pub fn do_cov(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let y_cdr = CDR(args);
    let y = if y_cdr.is_null() || y_cdr == R_NilValue() {
        R_NilValue()
    } else {
        CAR(y_cdr)
    };

    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarReal(NA_REAL);
    }

    let x_data = get_numeric_data(x);
    let y_data = if y.is_null() || y == R_NilValue() {
        x_data.clone()
    } else {
        get_numeric_data(y)
    };

    let n = x_data.len().min(y_data.len());
    if n == 0 {
        return Rf_ScalarReal(NA_REAL);
    }

    let mut sum_x = 0.0_f64;
    let mut sum_y = 0.0_f64;
    let mut count = 0_i64;
    for i in 0..n {
        if !x_data[i].is_nan() && !y_data[i].is_nan() {
            sum_x += x_data[i];
            sum_y += y_data[i];
            count += 1;
        }
    }
    if count < 2 {
        return Rf_ScalarReal(NA_REAL);
    }
    let mean_x = sum_x / count as f64;
    let mean_y = sum_y / count as f64;

    let mut cov = 0.0_f64;
    for i in 0..n {
        if !x_data[i].is_nan() && !y_data[i].is_nan() {
            cov += (x_data[i] - mean_x) * (y_data[i] - mean_y);
        }
    }
    Rf_ScalarReal(cov / (count as f64 - 1.0))
}

/// R's `cor(x, y)` — Pearson correlation between two numeric vectors.
pub fn do_cor(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let y_cdr = CDR(args);
    let y = if y_cdr.is_null() || y_cdr == R_NilValue() || CAR(y_cdr) == R_MissingArg() {
        R_NilValue()
    } else {
        CAR(y_cdr)
    };

    if x.is_null() || x == R_NilValue() || x == R_MissingArg() {
        return Rf_ScalarReal(NA_REAL);
    }

    let mut extra = if y_cdr.is_null() || y_cdr == R_NilValue() {
        R_NilValue()
    } else {
        CDR(y_cdr)
    };
    let mut position = 2;
    let mut use_mode = "everything".to_string();
    let mut method_mode = "pearson".to_string();
    while !extra.is_null() && extra != R_NilValue() {
        let name = tag_name(extra).unwrap_or_else(|| {
            if position == 2 {
                "use".into()
            } else if position == 3 {
                "method".into()
            } else {
                "unknown".into()
            }
        });
        let value = CAR(extra);
        if name == "method" {
            if value != R_MissingArg()
                && (TYPEOF(value) != SEXPTYPE::STRSXP || XLENGTH(value) != 1 || {
                    let method = charsxp_text(STRING_ELT(value, 0)).unwrap_or_default();
                    method.as_bytes() != b"pearson" && method.as_bytes() != b"spearman"
                })
            {
                base_error("unsupported cor method");
            }
            if value != R_MissingArg() {
                method_mode = charsxp_text(STRING_ELT(value, 0)).unwrap_or_default();
            }
        } else if name == "use" {
            if value != R_MissingArg() {
                if TYPEOF(value) != SEXPTYPE::STRSXP || XLENGTH(value) != 1 {
                    base_error("invalid 'use' argument");
                }
                use_mode = charsxp_text(STRING_ELT(value, 0)).unwrap_or_default();
                if !matches!(
                    use_mode.as_str(),
                    "everything"
                        | "all.obs"
                        | "complete.obs"
                        | "na.or.complete"
                        | "pairwise.complete.obs"
                ) {
                    base_error("invalid 'use' argument");
                }
            }
        } else {
            base_error("unsupported cor argument");
        }
        position += 1;
        extra = CDR(extra);
    }

    // Matrices are column-major in R.  Support Pearson correlation for
    // finite numeric columns while retaining the scalar vector path below.
    let dims = |value: SEXP| -> Option<(usize, usize)> {
        let dim =
            crate::sexp::attrib_core::getAttrib(value, crate::sexp::attrib_core::R_DimSymbol());
        if TYPEOF(dim) != SEXPTYPE::INTSXP || XLENGTH(dim) != 2 {
            return None;
        }
        let rows = integer_elt_checked(dim, 0) as isize;
        let cols = integer_elt_checked(dim, (1) as R_xlen_t) as isize;
        (rows >= 0 && cols >= 0).then_some((rows as usize, cols as usize))
    };
    let x_dims = dims(x);
    let y_missing = y.is_null() || y == R_NilValue() || y == R_MissingArg();
    let y_dims = if y.is_null() || y == R_NilValue() || y == R_MissingArg() {
        None
    } else {
        dims(y)
    };
    if x_dims.is_some() || y_dims.is_some() {
        if use_mode != "everything" || method_mode != "pearson" {
            base_error("matrix cor currently supports only use='everything', method='pearson'");
        }
        let (x_rows, nx) = x_dims.unwrap_or((XLENGTH(x) as usize, 1));
        let (y_rows, ny) = match y_dims {
            Some((rows, cols)) => (rows, cols),
            None if x_dims.is_some() && y_missing => (x_rows, nx),
            None => (x_rows, 1),
        };
        if y_rows != x_rows {
            base_error(format!("incompatible dimensions ({x_rows} vs {y_rows})"));
        }
        let x_data = get_numeric_data(x);
        let y_data = if y.is_null() || y == R_NilValue() || y == R_MissingArg() {
            x_data.clone()
        } else {
            get_numeric_data(y)
        };
        // The matrix implementation intentionally covers the finite,
        // default Pearson case only.  Do not silently turn R's default
        // `use = "everything"` NA semantics into pairwise deletion.
        if x_data.iter().any(|value| !value.is_finite())
            || y_data.iter().any(|value| !value.is_finite())
        {
            base_error("matrix cor currently requires finite numeric data");
        }
        let x_is_matrix = x_dims.is_some();
        let y_is_matrix = y_dims.is_some() || y_missing && x_is_matrix;
        if (!x_is_matrix && x_data.len() != x_rows) || (!y_is_matrix && y_data.len() != y_rows) {
            base_error("incompatible dimensions");
        }
        if x_rows.checked_mul(nx) != Some(x_data.len())
            || y_rows.checked_mul(ny) != Some(y_data.len())
        {
            base_error("matrix dimensions do not match correlation data");
        }
        let result_len = nx
            .checked_mul(ny)
            .unwrap_or_else(|| base_error("correlation matrix is too large"));
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, result_len as i64);
        let _guard = protect(result);
        for j in 0..ny {
            for i in 0..nx {
                let (mut sx, mut sy, mut count) = (0., 0., 0usize);
                for r in 0..x_rows {
                    let xv = x_data[if x_is_matrix { i * x_rows + r } else { r }];
                    let yv = y_data[if y_is_matrix { j * y_rows + r } else { r }];
                    if xv.is_finite() && yv.is_finite() {
                        sx += xv;
                        sy += yv;
                        count += 1;
                    }
                }
                let value = if count < 2 {
                    NA_REAL
                } else {
                    let (mx, my) = (sx / count as f64, sy / count as f64);
                    let (mut cov, mut vx, mut vy) = (0., 0., 0.);
                    for r in 0..x_rows {
                        let xv = x_data[if x_is_matrix { i * x_rows + r } else { r }];
                        let yv = y_data[if y_is_matrix { j * y_rows + r } else { r }];
                        if xv.is_finite() && yv.is_finite() {
                            let dx = xv - mx;
                            let dy = yv - my;
                            cov += dx * dy;
                            vx += dx * dx;
                            vy += dy * dy;
                        }
                    }
                    let denom = (vx * vy).sqrt();
                    if denom == 0. { NA_REAL } else { cov / denom }
                };
                set_real_checked(result, (j * nx + i) as R_xlen_t, value);
            }
        }
        // R returns a matrix whenever either argument is a matrix,
        // including matrix/vector and vector/matrix correlations.
        if x_is_matrix || y_is_matrix {
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            let _dim_guard = protect(dim);
            set_integer_checked(dim, 0, nx as i32);
            set_integer_checked(dim, (1) as R_xlen_t, ny as i32);
            crate::mainutils::relop::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimSymbol(),
                dim,
            );
            let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            let _dng = protect(dimnames);
            let column_names = |value: SEXP| {
                let dn = crate::sexp::attrib_core::getAttrib(
                    value,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                );
                if TYPEOF(dn) == SEXPTYPE::VECSXP && XLENGTH(dn) >= 2 {
                    VECTOR_ELT(dn, 1)
                } else {
                    R_NilValue()
                }
            };
            SET_VECTOR_ELT(
                dimnames,
                0,
                if x_is_matrix {
                    column_names(x)
                } else {
                    R_NilValue()
                },
            );
            SET_VECTOR_ELT(
                dimnames,
                1,
                if y_is_matrix {
                    if y.is_null() || y == R_NilValue() || y == R_MissingArg() {
                        column_names(x)
                    } else {
                        column_names(y)
                    }
                } else if y.is_null() || y == R_NilValue() || y == R_MissingArg() {
                    column_names(x)
                } else {
                    R_NilValue()
                },
            );
            crate::mainutils::relop::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dimnames,
            );
        }
        return result;
    }

    let x_data = get_numeric_data(x);
    let y_data = if y.is_null() || y == R_NilValue() || y == R_MissingArg() {
        x_data.clone()
    } else {
        get_numeric_data(y)
    };

    if x_data.len() != y_data.len() {
        base_error("incompatible lengths");
    }
    let n = x_data.len();
    if n == 0 {
        match use_mode.as_str() {
            "complete.obs" => base_error("no complete element pairs"),
            "pairwise.complete.obs" => base_error("'x' is empty"),
            _ => return Rf_ScalarReal(NA_REAL),
        }
    }

    let missing = |value: f64| value.is_nan();
    let missing_count = (0..n)
        .filter(|&i| missing(x_data[i]) || missing(y_data[i]))
        .count();
    match use_mode.as_str() {
        "everything" if missing_count > 0 => return Rf_ScalarReal(NA_REAL),
        "all.obs" if missing_count > 0 => base_error("missing observations in cov/cor"),
        _ => {}
    }

    let ranked: Option<(Vec<f64>, Vec<f64>)> = if method_mode == "spearman" {
        let active: Vec<usize> = (0..n)
            .filter(|&i| use_mode == "everything" || (!missing(x_data[i]) && !missing(y_data[i])))
            .collect();
        let rank = |values: &[f64]| -> Vec<f64> {
            let mut order: Vec<usize> = (0..values.len()).collect();
            order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
            let mut ranks = vec![0.0; values.len()];
            let mut start = 0;
            while start < order.len() {
                let mut end = start + 1;
                while end < order.len() && values[order[end]] == values[order[start]] {
                    end += 1;
                }
                let average = (start + end + 1) as f64 / 2.0;
                for &index in &order[start..end] {
                    ranks[index] = average;
                }
                start = end;
            }
            ranks
        };
        let xv: Vec<f64> = active.iter().map(|&i| x_data[i]).collect();
        let yv: Vec<f64> = active.iter().map(|&i| y_data[i]).collect();
        Some((rank(&xv), rank(&yv)))
    } else {
        None
    };

    let mut sum_x = 0.0_f64;
    let mut sum_y = 0.0_f64;
    let count = if let Some((calc_x, calc_y)) = ranked.as_ref() {
        for i in 0..calc_x.len() {
            sum_x += calc_x[i];
            sum_y += calc_y[i];
        }
        calc_x.len() as i64
    } else {
        let mut count = 0_i64;
        for i in 0..n {
            if use_mode == "everything" || (!missing(x_data[i]) && !missing(y_data[i])) {
                sum_x += x_data[i];
                sum_y += y_data[i];
                count += 1;
            }
        }
        count
    };
    if count < 2 {
        if count == 0 && use_mode == "complete.obs" {
            base_error("no complete element pairs");
        }
        return Rf_ScalarReal(NA_REAL);
    }
    let mean_x = sum_x / count as f64;
    let mean_y = sum_y / count as f64;

    let mut cov = 0.0_f64;
    let mut var_x = 0.0_f64;
    let mut var_y = 0.0_f64;
    if let Some((calc_x, calc_y)) = ranked.as_ref() {
        for i in 0..calc_x.len() {
            let dx = calc_x[i] - mean_x;
            let dy = calc_y[i] - mean_y;
            cov += dx * dy;
            var_x += dx * dx;
            var_y += dy * dy;
        }
    } else {
        for i in 0..n {
            if use_mode == "everything" || (!missing(x_data[i]) && !missing(y_data[i])) {
                let dx = x_data[i] - mean_x;
                let dy = y_data[i] - mean_y;
                cov += dx * dy;
                var_x += dx * dx;
                var_y += dy * dy;
            }
        }
    }
    let denom = (var_x * var_y).sqrt();
    if denom == 0.0 {
        crate::mainutils::errors::Rf_warningcall1(call, c"the standard deviation is zero");
        return Rf_ScalarReal(NA_REAL);
    }
    Rf_ScalarReal(cov / denom)
}

/// R's `scale(x, center=TRUE, scale=TRUE)` — standardize a numeric vector.
pub fn do_scale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = scale_argument(args, "x", 0);
    let center_arg = scale_argument(args, "center", 1);
    let scale_arg = scale_argument(args, "scale", 2);

    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
    let (nrow, ncol) = if !dim.is_null()
        && dim != R_NilValue()
        && TYPEOF(dim) == SEXPTYPE::INTSXP
        && XLENGTH(dim) >= 2
    {
        (
            integer_elt_checked(dim, 0) as R_xlen_t,
            integer_elt_checked(dim, (1) as R_xlen_t) as R_xlen_t,
        )
    } else {
        (XLENGTH(x), 1)
    };
    let n = nrow.saturating_mul(ncol);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = protect(result);
    let result_dim = if !dim.is_null() && dim != R_NilValue() {
        dim
    } else {
        let result_dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        set_integer_checked(result_dim, 0, nrow as c_int);
        set_integer_checked(result_dim, (1) as R_xlen_t, ncol as c_int);
        result_dim
    };
    let _result_dim_guard = protect(result_dim);
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_DimSymbol(),
        result_dim,
    );
    let input_dimnames =
        crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimNamesSymbol());
    let result_dimnames = if !input_dimnames.is_null() && input_dimnames != R_NilValue() {
        input_dimnames
    } else {
        let input_names = crate::sexp::attrib_core::getAttrib(x, install_str("names"));
        if input_names.is_null() || input_names == R_NilValue() {
            R_NilValue()
        } else {
            let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            let _dn = protect(dimnames);
            SET_VECTOR_ELT(dimnames, 0, input_names);
            SET_VECTOR_ELT(dimnames, 1, R_NilValue());
            dimnames
        }
    };
    let _result_dimnames_guard = protect(result_dimnames);
    if !result_dimnames.is_null() && result_dimnames != R_NilValue() {
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            result_dimnames,
        );
    }
    let center_numeric = !center_arg.is_null()
        && center_arg != R_NilValue()
        && (TYPEOF(center_arg) == SEXPTYPE::REALSXP || TYPEOF(center_arg) == SEXPTYPE::INTSXP);
    let scale_numeric = !scale_arg.is_null()
        && scale_arg != R_NilValue()
        && (TYPEOF(scale_arg) == SEXPTYPE::REALSXP || TYPEOF(scale_arg) == SEXPTYPE::INTSXP);
    let do_center = !center_numeric
        && (center_arg.is_null()
            || center_arg == R_NilValue()
            || (TYPEOF(center_arg) == SEXPTYPE::LGLSXP
                && logical_elt_checked(center_arg, 0) == TRUE));
    let do_scale = !scale_numeric
        && (scale_arg.is_null()
            || scale_arg == R_NilValue()
            || (TYPEOF(scale_arg) == SEXPTYPE::LGLSXP
                && logical_elt_checked(scale_arg, 0) == TRUE));
    let number_at = |arg: SEXP, j: R_xlen_t| -> f64 {
        let len = XLENGTH(arg).max(1);
        let i = j % len;
        if TYPEOF(arg) == SEXPTYPE::INTSXP {
            let v = integer_elt_checked(arg, (i as usize) as R_xlen_t);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            real_elt_checked(arg, (i as usize) as R_xlen_t)
        }
    };
    let mut dst = owner_sexp_mut(result);
    let mut centers = vec![0.0; ncol as usize];
    let mut scales = vec![1.0; ncol as usize];
    for j in 0..ncol {
        let mut sum = 0.0;
        let mut count = 0i64;
        for i in 0..nrow {
            let v = real_or_default(elt_to_sexp(x, i + j * nrow), NA_REAL);
            if v.is_finite() {
                sum += v;
                count += 1;
            }
        }
        let mean = if count > 0 { sum / count as f64 } else { 0.0 };
        let mut var_sum = 0.0;
        if do_scale {
            for i in 0..nrow {
                let v = real_or_default(elt_to_sexp(x, i + j * nrow), NA_REAL);
                if v.is_finite() {
                    var_sum += (v - mean) * (v - mean);
                }
            }
        }
        let sd = if count > 1 {
            (var_sum / (count as f64 - 1.0)).sqrt()
        } else {
            1.0
        };
        let cen = if center_numeric {
            number_at(center_arg, j)
        } else if do_center {
            mean
        } else {
            0.0
        };
        let sc = if scale_numeric {
            number_at(scale_arg, j)
        } else if do_scale && sd != 0.0 && sd.is_finite() {
            sd
        } else {
            1.0
        };
        centers[j as usize] = cen;
        scales[j as usize] = sc;
        for i in 0..nrow {
            let v = real_or_default(elt_to_sexp(x, i + j * nrow), NA_REAL);
            let centered = if cen.is_nan() { NA_REAL } else { v - cen };
            sexp_ok(dst.try_set_real_elt(
                ((i + j * nrow) as usize) as R_xlen_t,
                if sc.is_finite() && sc != 0.0 {
                    centered / sc
                } else {
                    centered
                },
            ));
        }
    }
    let colnames = if TYPEOF(result_dimnames) == SEXPTYPE::VECSXP && XLENGTH(result_dimnames) > 1 {
        VECTOR_ELT(result_dimnames, 1)
    } else {
        R_NilValue()
    };
    let set_scale_metadata = |name: &'static std::ffi::CStr, values: &[f64]| {
        let attribute = Rf_allocVector3(SEXPTYPE::REALSXP, values.len() as R_xlen_t);
        let _a = protect(attribute);
        for (i, value) in values.iter().enumerate() {
            set_real_checked(attribute, (i) as R_xlen_t, *value);
        }
        if TYPEOF(colnames) == SEXPTYPE::STRSXP && XLENGTH(colnames) == values.len() as R_xlen_t {
            crate::sexp::attrib_core::setAttrib(attribute, install_str("names"), colnames);
        }
        crate::sexp::attrib_core::setAttrib(result, install_cstr(name), attribute);
    };
    if do_center || center_numeric {
        set_scale_metadata(c"scaled:center", &centers);
    }
    if do_scale || scale_numeric {
        set_scale_metadata(c"scaled:scale", &scales);
    }
    result
}

fn scale_argument(args: SEXP, formal: &str, position: usize) -> SEXP {
    {
        let mut partial = None;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if let Some(tag) = tag_name(current) {
                if tag == formal {
                    return CAR(current);
                }
                if !tag.is_empty() && formal.starts_with(&tag) {
                    if partial.is_some() {
                        partial = None;
                        break;
                    }
                    partial = Some(CAR(current));
                }
            }
            current = CDR(current);
        }
        partial.unwrap_or_else(|| arg_by_name_or_position(args, &[formal], position))
    }
}

/// R's `rle(x)` — run-length encoding.
pub fn do_rle(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = arg_by_name_or_position(args, &["x"], 0);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
    let has_dim = !dim.is_null() && dim != R_NilValue();
    let atomic = matches!(
        TYPEOF(x),
        t if t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::RAWSXP
    );
    if has_dim || !(atomic || TYPEOF(x) == SEXPTYPE::VECSXP) {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "'x' must be a vector of an atomic type",
        );
    }

    let n = XLENGTH(x);
    if n == 0 {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_allocVector3(SEXPTYPE::INTSXP, 0));
        SET_VECTOR_ELT(result, 1, Rf_allocVector3(TYPEOF(x), 0));
        set_rle_attrs(result);
        return result;
    }

    // Collect run lengths and starting indices. Missing values are never
    // equal to the previous value in GNU R's rle().
    let mut lengths: Vec<i32> = Vec::new();
    let mut value_indices: Vec<R_xlen_t> = Vec::new();

    value_indices.push(0);
    lengths.push(1);

    for i in 1..n {
        let last_start = *value_indices.last().unwrap_or(&0);
        if rle_values_equal(x, i, last_start) {
            let last_idx = lengths.len() - 1;
            lengths[last_idx] += 1;
        } else {
            value_indices.push(i);
            lengths.push(1);
        }
    }

    let n_runs = lengths.len() as R_xlen_t;
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = protect(result);

    let lengths_sexp = Rf_allocVector3(SEXPTYPE::INTSXP, n_runs);
    let values_sexp = Rf_allocVector3(TYPEOF(x), n_runs);
    let _p2 = protect(lengths_sexp);
    let _p3 = protect(values_sexp);

    let mut dst_l = owner_sexp_mut(lengths_sexp);
    for i in 0..n_runs {
        sexp_ok(dst_l.try_set_integer_elt((i as usize) as R_xlen_t, lengths[i as usize]));
        copy_vector_elt(values_sexp, i, x, value_indices[i as usize]);
    }

    SET_VECTOR_ELT(result, 0, lengths_sexp);
    SET_VECTOR_ELT(result, 1, values_sexp);
    set_rle_attrs(result);
    result
}

fn set_rle_attrs(x: SEXP) {
    set_string_names(x, &["lengths".to_string(), "values".to_string()]);
    let class = mk_string_cstr(c"rle");
    if !class.is_null() {
        let _class_guard = protect(class);
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
    }
}

fn rle_values_equal(x: SEXP, lhs: R_xlen_t, rhs: R_xlen_t) -> bool {
    
        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP => {
                let a = integer_elt_checked(x, (lhs as usize) as R_xlen_t);
                let b = integer_elt_checked(x, (rhs as usize) as R_xlen_t);
                a != NA_INTEGER && b != NA_INTEGER && a == b
            }
            t if t == SEXPTYPE::REALSXP => {
                let a = real_elt_checked(x, (lhs as usize) as R_xlen_t);
                let b = real_elt_checked(x, (rhs as usize) as R_xlen_t);
                !ISNAN(a) && !ISNAN(b) && a == b
            }
            t if t == SEXPTYPE::STRSXP => {
                let a = STRING_ELT(x, lhs);
                let b = STRING_ELT(x, rhs);
                a != crate::sexp::globals::R_NaString()
                    && b != crate::sexp::globals::R_NaString()
                    && elt_to_string(x, lhs) == elt_to_string(x, rhs)
            }
            t if t == SEXPTYPE::RAWSXP => raw_elt_checked(x, (lhs as usize) as R_xlen_t) == raw_elt_checked(x, (rhs as usize) as R_xlen_t),
            _ => false,
        }
    
}

/// R's `inverse.rle(x)` — inverse of run-length encoding.
pub fn do_inverse_rle(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }

        let lengths_sexp = VECTOR_ELT(x, 0);
        let values_sexp = VECTOR_ELT(x, 1);
        if lengths_sexp.is_null() || values_sexp.is_null() {
            return R_NilValue();
        }

        let n_runs = XLENGTH(lengths_sexp);
        if n_runs == 0 {
            return Rf_allocVector3(TYPEOF(values_sexp), 0);
        }

        // Compute total length
        let mut total: R_xlen_t = 0;
        for i in 0..n_runs {
            total += (integer_elt_checked(lengths_sexp, (i as usize) as R_xlen_t)) as R_xlen_t;
        }

        let result = Rf_allocVector3(TYPEOF(values_sexp), total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let mut offset: R_xlen_t = 0;
        for i in 0..n_runs {
            let len = integer_elt_checked(lengths_sexp, (i as usize) as R_xlen_t);
            for j in 0..len {
                copy_vector_elt(result, offset + j as R_xlen_t, values_sexp, i);
            }
            offset += len as R_xlen_t;
        }
        result
    
}

/// GNU `rowsum(x, group)` — sum rows by group.
pub fn do_rowsum(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let group = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let xt = TYPEOF(x);
    if xt == SEXPTYPE::VECSXP {
        let class = crate::sexp::attrib_core::getAttrib(x, install_str("class"));
        let is_df = !class.is_null()
            && TYPEOF(class) == SEXPTYPE::STRSXP
            && (0..XLENGTH(class)).any(|i| {
                let s = STRING_ELT(class, i);
                !s.is_null()
                    && charsxp_cstring(s).to_str().ok() == Some("data.frame")
            });
        if !is_df {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'x' must be numeric",
            );
        }
        let nc = XLENGTH(x);
        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, nc);
        let _a = protect(ans);
        let mut rownames = R_NilValue();
        for i in 0..nc {
            let col = VECTOR_ELT(x, i);
            let args_i = Rf_cons(col, Rf_cons(group, R_NilValue()));
            let _ai = protect(args_i);
            let summed = do_rowsum(_call, _op, args_i, _rho);
            if i == 0 {
                let dn = crate::sexp::attrib_core::getAttrib(
                    summed,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                );
                if !dn.is_null() && TYPEOF(dn) == SEXPTYPE::VECSXP && XLENGTH(dn) > 0 {
                    rownames = VECTOR_ELT(dn, 0);
                }
            }
            crate::sexp::attrib_core::setAttrib(
                summed,
                crate::sexp::attrib_core::R_DimSymbol(),
                R_NilValue(),
            );
            crate::sexp::attrib_core::setAttrib(
                summed,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                R_NilValue(),
            );
            SET_VECTOR_ELT(ans, i, summed);
        }
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                ans,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        if !rownames.is_null() && rownames != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                ans,
                install_str("row.names"),
                rownames,
            );
        }
        let cls = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let c = std::ffi::CString::new("data.frame").unwrap_or_default();
        SET_STRING_ELT(cls, 0, mk_char_cstr(&c));
        crate::sexp::attrib_core::setAttrib(ans, install_str("class"), cls);
        return ans;
    }
    if xt != SEXPTYPE::INTSXP && xt != SEXPTYPE::REALSXP && xt != SEXPTYPE::LGLSXP {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "'x' must be numeric",
        );
    }
    let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
    let (nr, nc) = if !dim.is_null()
        && dim != R_NilValue()
        && TYPEOF(dim) == SEXPTYPE::INTSXP
        && XLENGTH(dim) >= 2
    {
        (integer_elt_checked(dim, 0) as i64, integer_elt_checked(dim, (1) as R_xlen_t) as i64)
    } else {
        (XLENGTH(x), 1)
    };
    if XLENGTH(group) != nr {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "incorrect length for 'group'",
        );
    }
    let mut labels: Vec<String> = Vec::new();
    let mut index: Vec<usize> = Vec::new();
    for i in 0..nr {
        let key = format_rowsum_group(group, i);
        if let Some(pos) = labels.iter().position(|s| s == &key) {
            index.push(pos);
        } else {
            index.push(labels.len());
            labels.push(key);
        }
    }
    let mut order: Vec<usize> = (0..labels.len()).collect();
    order.sort_by(|&a, &b| labels[a].cmp(&labels[b]));
    let mut new_pos = vec![0usize; labels.len()];
    let mut sorted_labels = vec![String::new(); labels.len()];
    for (dst, &src) in order.iter().enumerate() {
        new_pos[src] = dst;
        sorted_labels[dst].clone_from(&labels[src]);
    }
    let ng = labels.len() as i64;
    let out_ty = if xt == SEXPTYPE::REALSXP {
        SEXPTYPE::REALSXP
    } else {
        SEXPTYPE::INTSXP
    };
    let result = Rf_allocVector3(out_ty, ng * nc);
    let _result = protect(result);
    if out_ty == SEXPTYPE::REALSXP {
        for i in 0..(ng * nc) as usize {
            set_real_checked(result, (i) as R_xlen_t, 0.0);
        }
    } else {
        for i in 0..(ng * nc) as usize {
            set_integer_checked(result, (i) as R_xlen_t, 0);
        }
    }
    for col in 0..nc {
        for row in 0..nr {
            let g = new_pos[index[row as usize]] as i64;
            let src = row + col * nr;
            let dst = g + col * ng;
            let val = if xt == SEXPTYPE::REALSXP {
                real_elt_checked(x, (src as usize) as R_xlen_t)
            } else {
                integer_elt_checked(x, (src as usize) as R_xlen_t) as f64
            };
            if out_ty == SEXPTYPE::REALSXP {
                set_real_checked(result, (dst as usize) as R_xlen_t, real_elt_checked(result, (dst as usize) as R_xlen_t) + val);
            } else {
                set_integer_checked(result, (dst as usize) as R_xlen_t, integer_elt_checked(result, (dst as usize) as R_xlen_t) + val as i32);
            }
        }
    }
    let out_dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    set_integer_checked(out_dim, 0, ng as c_int);
    set_integer_checked(out_dim, (1) as R_xlen_t, nc as c_int);
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_DimSymbol(),
        out_dim,
    );
    let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    let rn = Rf_allocVector3(SEXPTYPE::STRSXP, ng);
    for (i, lab) in sorted_labels.iter().enumerate() {
        let cstr = CString::new(lab.as_str()).unwrap_or_default();
        SET_STRING_ELT(rn, i as i64, mk_char_cstr(&cstr));
    }
    SET_VECTOR_ELT(dn, 0, rn);
    let xdn =
        crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimNamesSymbol());
    let cn = if !xdn.is_null()
        && xdn != R_NilValue()
        && TYPEOF(xdn) == SEXPTYPE::VECSXP
        && XLENGTH(xdn) > 1
    {
        VECTOR_ELT(xdn, 1)
    } else {
        R_NilValue()
    };
    SET_VECTOR_ELT(dn, 1, cn);
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_DimNamesSymbol(),
        dn,
    );
    result
}

fn format_rowsum_group(group: SEXP, i: i64) -> String {
    
        // GNU rowsum.default converts a factor `group` to character
        // first (`group <- as.character(group)`); the integer codes
        // must be mapped through `levels()` for the row labels.
        if crate::mainutils::subset::has_class_factor(group) {
            let levels = crate::sexp::attrib_core::getAttrib(
                group,
                crate::sexp::attrib_core::R_LevelsSymbol(),
            );
            if !levels.is_null() && levels != R_NilValue() && TYPEOF(levels) == SEXPTYPE::STRSXP {
                let code = integer_elt_checked(group, (i as usize) as R_xlen_t);
                if code == crate::sexp::ffi::NA_INTEGER {
                    return "NA".into();
                }
                if code >= 1 && (code as i64) <= XLENGTH(levels) {
                    let s = STRING_ELT(levels, (code - 1) as i64);
                    if !s.is_null() {
                        return charsxp_text(s).unwrap_or_default();
                    }
                }
                return String::new();
            }
        }
        match TYPEOF(group) {
            t if t == SEXPTYPE::STRSXP => {
                let s = STRING_ELT(group, i);
                if s.is_null() {
                    "NA".into()
                } else {
                    charsxp_text(s).unwrap_or_default()
                }
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                format!("{}", integer_elt_checked(group, (i as usize) as R_xlen_t))
            }
            t if t == SEXPTYPE::REALSXP => format!("{}", real_elt_checked(group, (i as usize) as R_xlen_t)),
            _ => i.to_string(),
        }
    
}

/// GNU `jitter(x, factor=1, amount=NULL)`.
pub fn do_jitter(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || XLENGTH(x) == 0 {
            return x;
        }
        let xt = TYPEOF(x);
        if xt != SEXPTYPE::INTSXP && xt != SEXPTYPE::REALSXP && xt != SEXPTYPE::LGLSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'x' must be numeric",
            );
        }
        let n = XLENGTH(x);
        let mut finite: Vec<f64> = Vec::new();
        for i in 0..n {
            let v = if xt == SEXPTYPE::REALSXP {
                real_elt_checked(x, (i as usize) as R_xlen_t)
            } else {
                integer_elt_checked(x, (i as usize) as R_xlen_t) as f64
            };
            if v.is_finite() {
                finite.push(v);
            }
        }
        let (lo, hi) = finite
            .iter()
            .copied()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                (a.min(v), b.max(v))
            });
        let mut z = hi - lo;
        if z == 0.0 {
            z = lo.abs();
        }
        if z == 0.0 {
            z = 1.0;
        }
        let mut factor = 1.0;
        let mut amount_arg = R_NilValue();
        let mut cell = CDR(args);
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                charsxp_text(PRINTNAME(tag)).unwrap_or_default()
            } else {
                String::new()
            };
            let slot = if name == "factor" {
                0
            } else if name == "amount" {
                1
            } else {
                let s = pos;
                pos += 1;
                s
            };
            if slot == 0 {
                factor = if TYPEOF(CAR(cell)) == SEXPTYPE::REALSXP {
                    real_elt_checked(CAR(cell), 0)
                } else if TYPEOF(CAR(cell)) == SEXPTYPE::INTSXP {
                    integer_elt_checked(CAR(cell), 0) as f64
                } else {
                    1.0
                };
            } else if slot == 1 {
                amount_arg = CAR(cell);
            }
            cell = CDR(cell);
        }
        let amount = if amount_arg.is_null() || amount_arg == R_NilValue() {
            let digits = 3 - (z.log10().floor() as i32);
            let p = 10f64.powi(digits);
            let mut rounded: Vec<f64> = finite.iter().map(|v| (v * p).round() / p).collect();
            rounded.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            rounded.dedup();
            let d = if rounded.len() >= 2 {
                rounded
                    .windows(2)
                    .map(|w| w[1] - w[0])
                    .fold(f64::INFINITY, f64::min)
            } else if rounded.first().copied().unwrap_or(0.0) != 0.0 {
                rounded[0] / 10.0
            } else {
                z / 10.0
            };
            factor / 5.0 * d.abs()
        } else {
            let a = if TYPEOF(amount_arg) == SEXPTYPE::REALSXP {
                real_elt_checked(amount_arg, 0)
            } else if TYPEOF(amount_arg) == SEXPTYPE::INTSXP {
                integer_elt_checked(amount_arg, 0) as f64
            } else {
                0.0
            };
            if a == 0.0 { factor * (z / 50.0) } else { a }
        };
        let n_s = Rf_ScalarInteger(n as c_int);
        let _n = protect(n_s);
        let a_s = Rf_ScalarReal(-1.0);
        let _a = protect(a_s);
        let b_s = Rf_ScalarReal(1.0);
        let _b = protect(b_s);
        let u = crate::library::stats::random::do_runif(n_s, a_s, b_s);
        let _u = protect(u);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let xv = if xt == SEXPTYPE::REALSXP {
                real_elt_checked(x, (i as usize) as R_xlen_t)
            } else {
                integer_elt_checked(x, (i as usize) as R_xlen_t) as f64
            };
            let uv = if TYPEOF(u) == SEXPTYPE::REALSXP {
                real_elt_checked(u, (i as usize) as R_xlen_t)
            } else {
                0.0
            };
            set_real_checked(result, (i as usize) as R_xlen_t, xv + amount * uv);
        }
        result
    
}

/// GNU `margin.table(x, margin)`.
pub fn do_margin_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    
        let x = CAR(args);
        let margin_arg = CAR(CDR(args));
        let xt = TYPEOF(x);
        if xt != SEXPTYPE::INTSXP && xt != SEXPTYPE::REALSXP && xt != SEXPTYPE::LGLSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'x' is not an array",
            );
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (nr, nc) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (integer_elt_checked(dim, 0) as i64, integer_elt_checked(dim, (1) as R_xlen_t) as i64)
        } else {
            (XLENGTH(x), 1)
        };
        let has_margin =
            !margin_arg.is_null() && margin_arg != R_NilValue() && XLENGTH(margin_arg) > 0;
        if !has_margin {
            let mut acc = 0.0;
            let n = XLENGTH(x);
            for i in 0..n {
                acc += if xt == SEXPTYPE::REALSXP {
                    real_elt_checked(x, (i as usize) as R_xlen_t)
                } else {
                    integer_elt_checked(x, (i as usize) as R_xlen_t) as f64
                };
            }
            return if xt == SEXPTYPE::REALSXP {
                Rf_ScalarReal(acc)
            } else {
                Rf_ScalarInteger(acc as c_int)
            };
        }
        let margin = if TYPEOF(margin_arg) == SEXPTYPE::INTSXP {
            integer_elt_checked(margin_arg, 0)
        } else if TYPEOF(margin_arg) == SEXPTYPE::REALSXP {
            real_elt_checked(margin_arg, 0) as c_int
        } else {
            1
        };
        let out_n = if margin == 1 { nr } else { nc };
        let result = Rf_allocVector3(
            if xt == SEXPTYPE::REALSXP {
                SEXPTYPE::REALSXP
            } else {
                SEXPTYPE::INTSXP
            },
            out_n,
        );
        let _r = protect(result);
        if xt == SEXPTYPE::REALSXP {
            for i in 0..out_n as usize {
                set_real_checked(result, (i) as R_xlen_t, 0.0);
            }
        } else {
            for i in 0..out_n as usize {
                set_integer_checked(result, (i) as R_xlen_t, 0);
            }
        }
        for col in 0..nc {
            for row in 0..nr {
                let src = row + col * nr;
                let val = if xt == SEXPTYPE::REALSXP {
                    real_elt_checked(x, (src as usize) as R_xlen_t)
                } else {
                    integer_elt_checked(x, (src as usize) as R_xlen_t) as f64
                };
                let dst = if margin == 1 { row } else { col };
                if xt == SEXPTYPE::REALSXP {
                    set_real_checked(result, (dst as usize) as R_xlen_t, real_elt_checked(result, (dst as usize) as R_xlen_t) + val);
                } else {
                    set_integer_checked(result, (dst as usize) as R_xlen_t, integer_elt_checked(result, (dst as usize) as R_xlen_t) + val as i32);
                }
            }
        }
        let out_dim = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
        set_integer_checked(out_dim, 0, out_n as c_int);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimSymbol(),
            out_dim,
        );
        result
    
}

/// GNU `mad(x)` — median absolute deviation times 1.4826.
pub fn do_mad(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(NA_REAL);
        }
        let xt = TYPEOF(x);
        if xt != SEXPTYPE::INTSXP && xt != SEXPTYPE::REALSXP && xt != SEXPTYPE::LGLSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'x' must be numeric",
            );
        }
        let mut constant = 1.4826;
        let mut cell = CDR(args);
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                charsxp_text(PRINTNAME(tag)).unwrap_or_default()
            } else {
                String::new()
            };
            if name == "constant" || (name.is_empty() && pos == 1) {
                let v = CAR(cell);
                if TYPEOF(v) == SEXPTYPE::REALSXP {
                    constant = real_elt_checked(v, 0);
                } else if TYPEOF(v) == SEXPTYPE::INTSXP {
                    constant = integer_elt_checked(v, 0) as f64;
                }
            }
            if name.is_empty() {
                pos += 1;
            }
            cell = CDR(cell);
        }
        let med_args = Rf_cons(x, R_NilValue());
        let _ma = protect(med_args);
        let center = crate::mainutils::essentials::do_median(call, op, med_args, rho);
        let _c = protect(center);
        let cval = if TYPEOF(center) == SEXPTYPE::REALSXP {
            real_elt_checked(center, 0)
        } else if TYPEOF(center) == SEXPTYPE::INTSXP {
            integer_elt_checked(center, 0) as f64
        } else {
            NA_REAL
        };
        let n = XLENGTH(x);
        let absv = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _a = protect(absv);
        for i in 0..n {
            let xv = if xt == SEXPTYPE::REALSXP {
                real_elt_checked(x, (i as usize) as R_xlen_t)
            } else {
                integer_elt_checked(x, (i as usize) as R_xlen_t) as f64
            };
            set_real_checked(absv, (i as usize) as R_xlen_t, (xv - cval).abs());
        }
        let abs_args = Rf_cons(absv, R_NilValue());
        let _aa = protect(abs_args);
        let med = crate::mainutils::essentials::do_median(call, op, abs_args, rho);
        let mval = if TYPEOF(med) == SEXPTYPE::REALSXP {
            real_elt_checked(med, 0)
        } else {
            0.0
        };
        Rf_ScalarReal(constant * mval)
    
}

/// GNU `fivenum(x)` — Tukey five-number summary.
pub fn do_fivenum(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, 5);
            for i in 0..5 {
                set_real_checked(result, (i) as R_xlen_t, NA_REAL);
            }
            return result;
        }
        let xt = TYPEOF(x);
        let n0 = XLENGTH(x);
        let mut vals: Vec<f64> = Vec::new();
        for i in 0..n0 {
            let v = if xt == SEXPTYPE::REALSXP {
                real_elt_checked(x, (i as usize) as R_xlen_t)
            } else if xt == SEXPTYPE::INTSXP || xt == SEXPTYPE::LGLSXP {
                let iv = integer_elt_checked(x, (i as usize) as R_xlen_t);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            if v.is_finite() {
                vals.push(v);
            }
        }
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = vals.len();
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 5);
        if n == 0 {
            for i in 0..5 {
                set_real_checked(result, (i) as R_xlen_t, NA_REAL);
            }
            return result;
        }
        let n4 = (((n + 3) / 2) as f64) / 2.0;
        let d = [
            1.0,
            n4,
            (n as f64 + 1.0) / 2.0,
            n as f64 + 1.0 - n4,
            n as f64,
        ];
        for (i, di) in d.iter().enumerate() {
            let lo = di.floor() as usize;
            let hi = di.ceil() as usize;
            let a = vals[lo.saturating_sub(1).min(n - 1)];
            let b = vals[hi.saturating_sub(1).min(n - 1)];
            set_real_checked(result, (i) as R_xlen_t, 0.5 * (a + b));
        }
        result
    
}

/// GNU `boxplot.stats(x, coef=1.5)`.
pub fn do_boxplot_stats(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    
        let x = CAR(args);
        let mut coef = 1.5;
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let c = CAR(rest);
            if !c.is_null() && c != R_NilValue() {
                coef = elt_real_safe(c, 0);
            }
        }
        let stats = do_fivenum(_call, _op, Rf_cons(x, R_NilValue()), _rho);
        let _s = protect(stats);
        let q1 = real_elt_checked(stats, (1) as R_xlen_t);
        let med = real_elt_checked(stats, (2) as R_xlen_t);
        let q3 = real_elt_checked(stats, (3) as R_xlen_t);
        let iqr = q3 - q1;
        let n0 = if x.is_null() || x == R_NilValue() {
            0
        } else {
            XLENGTH(x)
        };
        let xt = if x.is_null() || x == R_NilValue() {
            SEXPTYPE::REALSXP
        } else {
            SEXPTYPE(TYPEOF(x))
        };
        let mut n = 0i32;
        let mut outs: Vec<f64> = Vec::new();
        let mut insides: Vec<f64> = Vec::new();
        for i in 0..n0 {
            let v = if xt == SEXPTYPE::REALSXP {
                real_elt_checked(x, (i as usize) as R_xlen_t)
            } else if xt == SEXPTYPE::INTSXP || xt == SEXPTYPE::LGLSXP {
                let iv = integer_elt_checked(x, (i as usize) as R_xlen_t);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v.is_nan() {
                continue;
            }
            n += 1;
            let is_out =
                coef > 0.0 && iqr.is_finite() && (v < q1 - coef * iqr || v > q3 + coef * iqr);
            if is_out {
                outs.push(v);
            } else if v.is_finite() {
                insides.push(v);
            }
        }
        if !insides.is_empty() && coef > 0.0 {
            let lo = insides.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = insides.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            set_real_checked(stats, 0, lo);
            set_real_checked(stats, (4) as R_xlen_t, hi);
        }
        let conf = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _c = protect(conf);
        if n > 0 && iqr.is_finite() {
            let half = 1.58 * iqr / (n as f64).sqrt();
            set_real_checked(conf, 0, med - half);
            set_real_checked(conf, (1) as R_xlen_t, med + half);
        } else {
            set_real_checked(conf, 0, NA_REAL);
            set_real_checked(conf, (1) as R_xlen_t, NA_REAL);
        }
        let out = Rf_allocVector3(SEXPTYPE::REALSXP, outs.len() as i64);
        let _o = protect(out);
        for (i, v) in outs.iter().enumerate() {
            set_real_checked(out, (i) as R_xlen_t, *v);
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, stats);
        SET_VECTOR_ELT(result, 1, Rf_ScalarInteger(n));
        SET_VECTOR_ELT(result, 2, conf);
        SET_VECTOR_ELT(result, 3, out);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "stats".to_string(),
                "n".to_string(),
                "conf".to_string(),
                "out".to_string(),
            ],
        );
        result
    
}

pub(super) fn p_adjust_values(p: &[f64], method: &str) -> Vec<f64> {
    let n = p.len();
    if n <= 1 {
        return p.to_vec();
    }
    match method {
        "none" => p.to_vec(),
        "bonferroni" => p.iter().map(|v| (n as f64 * v).min(1.0)).collect(),
        "BH" | "fdr" => {
            let mut o: Vec<usize> = (0..n).collect();
            o.sort_by(|&a, &b| p[b].partial_cmp(&p[a]).unwrap_or(std::cmp::Ordering::Equal));
            let mut adj = vec![0.0; n];
            let mut running: f64 = 1.0;
            for (rank_from_end, &idx) in o.iter().enumerate() {
                let i = n - rank_from_end;
                let val = ((n as f64 / i as f64) * p[idx]).min(1.0);
                running = running.min(val);
                adj[idx] = running;
            }
            adj
        }
        _ => {
            // holm
            let mut o: Vec<usize> = (0..n).collect();
            o.sort_by(|&a, &b| p[a].partial_cmp(&p[b]).unwrap_or(std::cmp::Ordering::Equal));
            let mut adj = vec![0.0; n];
            let mut running: f64 = 0.0;
            for (i, &idx) in o.iter().enumerate() {
                let val = (((n - i) as f64) * p[idx]).min(1.0);
                running = running.max(val);
                adj[idx] = running;
            }
            adj
        }
    }
}

/// GNU `p.adjust(p, method)`.
pub fn do_p_adjust(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let mut method = "holm".to_string();
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let m = CAR(rest);
            if TYPEOF(m) == SEXPTYPE::STRSXP && XLENGTH(m) > 0 {
                method = elt_to_string(m, 0);
            }
        }
        let n = XLENGTH(x);
        let mut p = Vec::with_capacity(n as usize);
        for i in 0..n {
            p.push(elt_real_safe(x, i));
        }
        let adj = p_adjust_values(&p, &method);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for (i, v) in adj.iter().enumerate() {
            set_real_checked(result, (i) as R_xlen_t, *v);
        }
        result
    
}
