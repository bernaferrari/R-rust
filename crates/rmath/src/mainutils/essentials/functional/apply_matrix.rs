//! apply/tapply/outer/kronecker matrix iteration family.

use super::*;

pub(super) fn list_apply_len(x: SEXP) -> R_xlen_t {
    {
        match TYPEOF(x) {
            t if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP => {
                let mut len = 0;
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    len += 1;
                    current = CDR(current);
                }
                len
            }
            _ => XLENGTH(x),
        }
    }
}

pub(super) fn list_apply_names(x: SEXP, n: R_xlen_t) -> SEXP {
    if n <= 0 {
        return R_NilValue();
    }
    match TYPEOF(x) {
        t if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP => {
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            if names.is_null() {
                return R_NilValue();
            }
            let _names_guard = protect(names);
            let mut current = x;
            let mut i = 0;
            let mut any_name = false;
            while !current.is_null() && current != R_NilValue() && i < n {
                let tag = TAG(current);
                if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                    let printname = PRINTNAME(tag);
                    if !printname.is_null() && printname != R_NilValue() {
                        SET_STRING_ELT(names, i, printname);
                        any_name = true;
                    } else {
                        SET_STRING_ELT(names, i, mk_char_cstr(c""));
                    }
                } else {
                    SET_STRING_ELT(names, i, mk_char_cstr(c""));
                }
                i += 1;
                current = CDR(current);
            }
            if any_name { names } else { R_NilValue() }
        }
        _ => {
            let names = crate::sexp::attrib_core::getAttrib(
                x,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && XLENGTH(names) == n
            {
                names
            } else {
                R_NilValue()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// apply / tapply / mapply / outer / sweep — higher-order array functions
// ---------------------------------------------------------------------------

/// Extract a row from a matrix (column-major storage) as a length-ncol vector.
pub(super) fn extract_matrix_row(x: SEXP, nrow: R_xlen_t, ncol: R_xlen_t, row: R_xlen_t) -> SEXP {
    
        let t = TYPEOF(x);
        let result = Rf_allocVector3(t, ncol);
        if result.is_null() {
            return R_NilValue();
        }
        for j in 0..ncol {
            let src = (j * nrow + row) as usize;
            if t == SEXPTYPE::REALSXP {
                set_real_checked(result, (j as usize) as R_xlen_t, real_elt_checked(x, (src) as R_xlen_t));
            } else if t == SEXPTYPE::INTSXP {
                set_integer_checked(result, (j as usize) as R_xlen_t, integer_elt_checked(x, (src) as R_xlen_t));
            } else if t == SEXPTYPE::LGLSXP {
                set_logical_checked(result, (j as usize) as R_xlen_t, logical_elt_checked(x, (src) as R_xlen_t));
            }
        }
        result
    
}

/// Extract a column from a matrix (column-major storage) as a length-nrow vector.
pub(super) fn extract_matrix_col(x: SEXP, nrow: R_xlen_t, _ncol: R_xlen_t, col: R_xlen_t) -> SEXP {
    
        let t = TYPEOF(x);
        let result = Rf_allocVector3(t, nrow);
        if result.is_null() {
            return R_NilValue();
        }
        let offset = (col * nrow) as usize;
        if t == SEXPTYPE::REALSXP {
            for i in 0..nrow {
                set_real_checked(result, (i as usize) as R_xlen_t, real_elt_checked(x, (offset + i as usize) as R_xlen_t));
            }
        } else if t == SEXPTYPE::INTSXP {
            for i in 0..nrow {
                set_integer_checked(result, (i as usize) as R_xlen_t, integer_elt_checked(x, (offset + i as usize) as R_xlen_t));
            }
        } else if t == SEXPTYPE::LGLSXP {
            for i in 0..nrow {
                set_logical_checked(result, (i as usize) as R_xlen_t, logical_elt_checked(x, (offset + i as usize) as R_xlen_t));
            }
        }
        result
    
}

/// R's `apply(X, MARGIN, FUN)` — apply FUN over margins of array/matrix.
///
/// For a 2D matrix:
/// - MARGIN=1: apply FUN to each row, return vector of length nrow
/// - MARGIN=2: apply FUN to each column, return vector of length ncol
pub fn do_apply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = eval_arg_by_name_or_position(args, &["X"], 0, rho);
        let margin_arg = eval_arg_by_name_or_position(args, &["MARGIN"], 1, rho);
        let fun = apply_fun(_call, args, rho);

        if x.is_null() || x == R_NilValue() || fun.is_null() {
            return R_NilValue();
        }

        // Get dimensions
        let dim_attr = crate::sexp::attrib_core::getAttrib(x, install_str("dim"));
        if dim_attr.is_null() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP || LENGTH(dim_attr) < 2 {
            return R_NilValue(); // not a matrix/array
        }
        let nrow = integer_elt_checked(dim_attr, 0) as R_xlen_t;
        let ncol = integer_elt_checked(dim_attr, (1) as R_xlen_t) as R_xlen_t;
        let margin = real_or_default(margin_arg, 1.0) as i64;

        if margin == 1 {
            // Apply over rows
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, nrow);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for i in 0..nrow {
                let row_vec = extract_matrix_row(x, nrow, ncol, i);
                let call_args = Rf_cons(row_vec, R_NilValue());
                let call_sexp = Rf_cons(fun, call_args);
                if !call_sexp.is_null() {
                    crate::sexp::accessors::SET_TYPEOF(call_sexp, SEXPTYPE::LANGSXP.as_c_int());
                }
                let val = crate::eval::eval::Rf_eval(call_sexp, rho);
                crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
            }
            simplify_scalar_list(result)
        } else if margin == 2 {
            // Apply over columns
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncol);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for j in 0..ncol {
                let col_vec = extract_matrix_col(x, nrow, ncol, j);
                let call_args = Rf_cons(col_vec, R_NilValue());
                let call_sexp = Rf_cons(fun, call_args);
                if !call_sexp.is_null() {
                    crate::sexp::accessors::SET_TYPEOF(call_sexp, SEXPTYPE::LANGSXP.as_c_int());
                }
                let val = crate::eval::eval::Rf_eval(call_sexp, rho);
                crate::sexp::accessors::SET_VECTOR_ELT(result, j as i64, val);
            }
            simplify_scalar_list(result)
        } else {
            R_NilValue()
        }
    }
}

pub(super) fn apply_fun(call: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let evaluated = callable_arg_by_name_or_position(args, &["FUN"], 2, rho);
    if is_function(evaluated) {
        return evaluated;
    }
    // `apply(x, 1, diff)` inside a function whose formal is `diff = TRUE`
    // still means base::diff. GNU match.fun recovers the symbol and
    // skips the non-function binding.
    let uneval = CDR(call);
    let expr = arg_by_name_or_position(uneval, &["FUN"], 2);
    if !expr.is_null() && TYPEOF(expr) == SEXPTYPE::SYMSXP {
        let found = find_fun(expr, rho);
        if is_function(found) {
            return found;
        }
    }
    evaluated
}

pub(super) fn is_function(value: SEXP) -> bool {
    {
        !value.is_null()
            && value != R_NilValue()
            && value != crate::sexp::globals::R_UnboundValue()
            && (TYPEOF(value) == SEXPTYPE::CLOSXP
                || TYPEOF(value) == SEXPTYPE::BUILTINSXP
                || TYPEOF(value) == SEXPTYPE::SPECIALSXP)
    }
}

/// R's `tapply(X, INDEX, FUN)` — apply FUN to each group defined by INDEX.
///
/// Iterates unique values of INDEX, collects matching elements from X, calls FUN on each group.
pub fn do_tapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let index = CAR(CDR(args));
        let fun = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() || index.is_null() || fun.is_null() {
            return R_NilValue();
        }
        if let Some(result) = tapply_numeric_array(x, index, fun, _call) {
            return result;
        }

        let n = XLENGTH(x);
        let idx_n = XLENGTH(index);

        // Collect unique index values and group membership
        let mut group_keys: Vec<i64> = Vec::new();
        let mut group_map: std::collections::BTreeMap<i64, usize> =
            std::collections::BTreeMap::new();
        let mut groups: Vec<Vec<R_xlen_t>> = Vec::new();

        let idx_t = TYPEOF(index);
        for i in 0..n {
            let idx_i = if idx_n == 0 { 0 } else { i % idx_n };
            let key = if idx_t == SEXPTYPE::INTSXP || idx_t == SEXPTYPE::LGLSXP {
                integer_elt_checked(index, (idx_i as usize) as R_xlen_t) as i64
            } else if idx_t == SEXPTYPE::REALSXP {
                (real_elt_checked(index, (idx_i as usize) as R_xlen_t)).to_bits() as i64
            } else {
                idx_i as i64
            };

            if let Some(&g) = group_map.get(&key) {
                groups[g].push(i);
            } else {
                let g = groups.len();
                group_map.insert(key, g);
                group_keys.push(key);
                groups.push(vec![i]);
            }
        }

        let num_groups = groups.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, num_groups);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (g, indices) in groups.iter().enumerate() {
            let group_vec = Rf_allocVector3(TYPEOF(x), indices.len() as R_xlen_t);
            if !group_vec.is_null() {
                let t = TYPEOF(x);
                for (j, &src_i) in indices.iter().enumerate() {
                    if t == SEXPTYPE::REALSXP {
                        set_real_checked(group_vec, (j) as R_xlen_t, real_elt_checked(x, (src_i as usize) as R_xlen_t));
                    } else if t == SEXPTYPE::INTSXP {
                        set_integer_checked(group_vec, (j) as R_xlen_t, integer_elt_checked(x, (src_i as usize) as R_xlen_t));
                    } else if t == SEXPTYPE::LGLSXP {
                        set_logical_checked(group_vec, (j) as R_xlen_t, logical_elt_checked(x, (src_i as usize) as R_xlen_t));
                    }
                }
            }
            let call_args = Rf_cons(group_vec, R_NilValue());
            let call_sexp = Rf_cons(fun, call_args);
            if !call_sexp.is_null() {
                crate::sexp::accessors::SET_TYPEOF(call_sexp, SEXPTYPE::LANGSXP.as_c_int());
            }
            let val = crate::eval::eval::Rf_eval(call_sexp, rho);
            crate::sexp::accessors::SET_VECTOR_ELT(result, g as i64, val);
        }

        result
    }
}

pub(super) struct TapplyIndex {
    labels: Vec<String>,
    row_codes: Vec<Option<usize>>,
    dim_name: Option<String>,
}

pub(crate) fn tapply_numeric_array(x: SEXP, index: SEXP, fun: SEXP, call: SEXP) -> Option<SEXP> {
    
        let summary = aggregate_summary_fun(fun, call)?;
        let x_type = TYPEOF(x);
        if x_type != SEXPTYPE::INTSXP && x_type != SEXPTYPE::REALSXP {
            return None;
        }
        let n = XLENGTH(x);
        let indexes = tapply_indexes(index, n)?;
        if indexes.is_empty() || indexes.iter().any(|index| index.labels.is_empty()) {
            return None;
        }

        let dims = indexes
            .iter()
            .map(|index| index.labels.len())
            .collect::<Vec<_>>();
        let total_len = dims.iter().product::<usize>() as R_xlen_t;
        let mut states = vec![AggregateGroupState::new(); total_len as usize];

        for row in 0..n {
            let mut offset = 0_usize;
            let mut stride = 1_usize;
            let mut keep = true;
            for (index, dim) in indexes.iter().zip(dims.iter()) {
                match index.row_codes[row as usize] {
                    Some(code) => {
                        offset += code * stride;
                        stride *= *dim;
                    }
                    None => {
                        keep = false;
                        break;
                    }
                }
            }
            if keep {
                states[offset].record(tapply_value_at(x, x_type, row), summary);
            }
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, total_len);
        if result.is_null() {
            return Some(result);
        }
        let _result_guard = protect(result);
        for (i, state) in states.into_iter().enumerate() {
            set_real_checked(result, (i) as R_xlen_t, state.summarize(summary));
        }
        set_tapply_dim_attrs(result, &indexes);
        Some(result)
    
}

pub(super) fn tapply_indexes(index: SEXP, n: R_xlen_t) -> Option<Vec<TapplyIndex>> {
    if TYPEOF(index) == SEXPTYPE::VECSXP {
        let mut indexes = Vec::with_capacity(XLENGTH(index) as usize);
        let names =
            crate::sexp::attrib_core::getAttrib(index, crate::sexp::attrib_core::R_NamesSymbol());
        for i in 0..XLENGTH(index) {
            let dim_name = if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && XLENGTH(names) > i
            {
                let name = string_at_or_empty(names, i);
                if name.is_empty() { None } else { Some(name) }
            } else {
                None
            };
            indexes.push(tapply_one_index(VECTOR_ELT(index, i), n, dim_name)?);
        }
        Some(indexes)
    } else {
        Some(vec![tapply_one_index(index, n, None)?])
    }
}

pub(super) fn tapply_one_index(
    index: SEXP,
    n: R_xlen_t,
    dim_name: Option<String>,
) -> Option<TapplyIndex> {
    if index.is_null() || index == R_NilValue() || XLENGTH(index) == 0 {
        return None;
    }
    let index_type = TYPEOF(index);
    if index_type == SEXPTYPE::INTSXP {
        if let Some(levels) = aggregate_factor_levels(index) {
            let row_codes = (0..n)
                .map(|row| {
                    let value = integer_elt_checked(index, row % XLENGTH(index));
                    if value == NA_INTEGER || value <= 0 || value as usize > levels.len() {
                        None
                    } else {
                        Some((value - 1) as usize)
                    }
                })
                .collect();
            return Some(TapplyIndex {
                labels: levels,
                row_codes,
                dim_name,
            });
        }
        let mut labels = std::collections::BTreeSet::<i32>::new();
        let mut values = Vec::with_capacity(n as usize);
        for row in 0..n {
            let value = integer_elt_checked(index, row % XLENGTH(index));
            if value == NA_INTEGER {
                values.push(None);
            } else {
                labels.insert(value);
                values.push(Some(value));
            }
        }
        let labels = labels.into_iter().collect::<Vec<_>>();
        let positions = labels
            .iter()
            .enumerate()
            .map(|(i, value)| (*value, i))
            .collect::<BTreeMap<_, _>>();
        let row_codes = values
            .into_iter()
            .map(|value| value.and_then(|value| positions.get(&value).copied()))
            .collect();
        return Some(TapplyIndex {
            labels: labels.into_iter().map(|value| value.to_string()).collect(),
            row_codes,
            dim_name,
        });
    }

    if index_type == SEXPTYPE::STRSXP {
        let mut labels = std::collections::BTreeSet::<String>::new();
        let mut values = Vec::with_capacity(n as usize);
        for row in 0..n {
            let elt = STRING_ELT(index, row % XLENGTH(index));
            if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                values.push(None);
            } else {
                let value = charsxp_cstring(elt).to_string_lossy().into_owned();
                labels.insert(value.clone());
                values.push(Some(value));
            }
        }
        let labels = labels.into_iter().collect::<Vec<_>>();
        let positions = labels
            .iter()
            .enumerate()
            .map(|(i, value)| (value.clone(), i))
            .collect::<BTreeMap<_, _>>();
        let row_codes = values
            .into_iter()
            .map(|value| value.and_then(|value| positions.get(&value).copied()))
            .collect();
        return Some(TapplyIndex {
            labels,
            row_codes,
            dim_name,
        });
    }

    None
}

pub(super) fn tapply_value_at(x: SEXP, x_type: c_int, i: R_xlen_t) -> f64 {
    
        if x_type == SEXPTYPE::REALSXP {
            real_elt_checked(x, (i as usize) as R_xlen_t)
        } else {
            let value = integer_elt_checked(x, (i as usize) as R_xlen_t);
            if value == NA_INTEGER {
                NA_REAL
            } else {
                value as f64
            }
        }
    
}

pub(super) fn set_tapply_dim_attrs(result: SEXP, indexes: &[TapplyIndex]) {
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, indexes.len() as R_xlen_t);
    if dim.is_null() {
        return;
    }
    let _dim_guard = protect(dim);
    for (i, index) in indexes.iter().enumerate() {
        set_integer_checked(dim, (i) as R_xlen_t, index.labels.len() as i32);
    }
    crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_DimSymbol(), dim);

    let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, indexes.len() as R_xlen_t);
    if dimnames.is_null() {
        return;
    }
    let _dimnames_guard = protect(dimnames);
    for (i, index) in indexes.iter().enumerate() {
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, index.labels.len() as R_xlen_t);
        if names.is_null() {
            return;
        }
        let _names_guard = protect(names);
        for (j, label) in index.labels.iter().enumerate() {
            let label_c = CString::new(label.as_str()).unwrap_or_default();
            SET_STRING_ELT(names, j as R_xlen_t, mk_char_cstr(&label_c));
        }
        SET_VECTOR_ELT(dimnames, i as R_xlen_t, names);
    }
    let dimname_names = Rf_allocVector3(SEXPTYPE::STRSXP, indexes.len() as R_xlen_t);
    if !dimname_names.is_null() {
        let _dimname_names_guard = protect(dimname_names);
        for (i, index) in indexes.iter().enumerate() {
            let label_c =
                CString::new(index.dim_name.as_deref().unwrap_or("")).unwrap_or_default();
            SET_STRING_ELT(dimname_names, i as R_xlen_t, mk_char_cstr(&label_c));
        }
        crate::sexp::attrib_core::setAttrib(
            dimnames,
            crate::sexp::attrib_core::R_NamesSymbol(),
            dimname_names,
        );
    }
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_DimNamesSymbol(),
        dimnames,
    );
}

/// R's `outer(X, Y, FUN="*")` — outer product. Returns a matrix of length(X) x length(Y).
///
/// For each pair (x_i, y_j), computes FUN(x_i, y_j).
pub fn do_outer(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let fun_arg = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
            return R_NilValue();
        }
        let nx = XLENGTH(x);
        let ny = XLENGTH(y);
        let star = fun_arg.is_null()
            || fun_arg == R_NilValue()
            || (TYPEOF(fun_arg) == SEXPTYPE::STRSXP && elt_to_string(fun_arg, 0) == "*");
        let robj = if star {
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, nx * ny);
            let _result = protect(result);
            let mut dst = owner_sexp_mut(result);
            for i in 0..nx {
                let xi = elt_real_safe(x, i);
                for j in 0..ny {
                    sexp_ok(dst.try_set_real_elt(((j * nx + i) as usize) as R_xlen_t, xi * elt_real_safe(y, j)));
                }
            }
            result
        } else {
            let fun = resolve_outer_fun(fun_arg, rho);
            let _fun = protect(fun);
            let xrep = rep_times(x, ny);
            let _xrep = protect(xrep);
            let yrep = rep_each(y, nx);
            let _yrep = protect(yrep);
            // GNU: FUN(X, Y, ...) — extra tagged args such as sep=":".
            let extra = CDR(CDR(CDR(args)));
            let call = if extra.is_null() || extra == R_NilValue() {
                Rf_lang3(fun, xrep, yrep)
            } else {
                let extra_dup = crate::mainutils::duplicate::shallow_duplicate(extra);
                let _ed = protect(extra_dup);
                let call = Rf_cons(fun, Rf_cons(xrep, Rf_cons(yrep, extra_dup)));
                if !call.is_null() {
                    crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
                }
                call
            };
            let _call = protect(call);
            crate::eval::eval::Rf_eval(call, rho)
        };
        let _robj = protect(robj);
        let mut od = array_dims(x);
        od.extend(array_dims(y));
        set_int_dim(robj, &od);
        attach_outer_dimnames(robj, x, y);
        robj
    }
}

/// GNU `.kronecker(X, Y, FUN="*")` — `aperm(outer(X,Y,FUN))` then `dim <- dX*dY`.
pub fn do_kronecker(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = as_array_for_kronecker(CAR(args));
        let _x = protect(x);
        let y = as_array_for_kronecker(CAR(CDR(args)));
        let _y = protect(y);
        let fun_cell = CDR(CDR(args));
        let fun = if fun_cell.is_null() || fun_cell == R_NilValue() {
            mk_string_cstr(c"*")
        } else {
            CAR(fun_cell)
        };
        let _fun = protect(fun);
        let mut dx = array_dims(x);
        let mut dy = array_dims(y);
        if dx.len() < dy.len() {
            dx.resize(dy.len(), 1);
            set_int_dim(x, &dx);
        } else if dy.len() < dx.len() {
            dy.resize(dx.len(), 1);
            set_int_dim(y, &dy);
        }
        let outer_args = Rf_cons(x, Rf_cons(y, Rf_cons(fun, R_NilValue())));
        let _oa = protect(outer_args);
        let opobj = do_outer(call, op, outer_args, rho);
        let _op = protect(opobj);
        let k = dx.len();
        let perm = Rf_allocVector3(SEXPTYPE::INTSXP, (2 * k) as i64);
        let _perm = protect(perm);
        for i in 0..k {
            set_integer_checked(perm, (2 * i) as R_xlen_t, (k + i + 1) as c_int);
            set_integer_checked(perm, (2 * i + 1) as R_xlen_t, (i + 1) as c_int);
        }
        let aperm_args = Rf_cons(opobj, Rf_cons(perm, R_NilValue()));
        let _aa = protect(aperm_args);
        let permuted = crate::mainutils::array::do_aperm(call, op, aperm_args, rho);
        let _p = protect(permuted);
        let out_dim: Vec<c_int> = dx
            .iter()
            .zip(dy.iter())
            .map(|(a, b)| a.saturating_mul(*b))
            .collect();
        set_int_dim(permuted, &out_dim);
        permuted
    }
}

pub(super) fn as_array_for_kronecker(x: SEXP) -> SEXP {
    let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
    if !dim.is_null() && dim != R_NilValue() && TYPEOF(dim) == SEXPTYPE::INTSXP {
        return x;
    }
    let y = crate::mainutils::duplicate::duplicate(x);
    let _y = protect(y);
    set_int_dim(y, &[XLENGTH(y) as c_int]);
    y
}

pub(super) fn array_dims(x: SEXP) -> Vec<c_int> {
    
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() || TYPEOF(dim) != SEXPTYPE::INTSXP {
            return vec![XLENGTH(x) as c_int];
        }
        let n = XLENGTH(dim) as usize;
        (0..n).map(|i| integer_elt_checked(dim, (i) as R_xlen_t)).collect()
    
}

pub(super) fn set_int_dim(x: SEXP, dims: &[c_int]) {
    
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, dims.len() as i64);
        for (i, d) in dims.iter().enumerate() {
            set_integer_checked(dim, (i) as R_xlen_t, *d);
        }
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_DimSymbol(), dim);
    
}

pub(super) fn attach_outer_dimnames(robj: SEXP, x: SEXP, y: SEXP) {
    let nxn = crate::attrib_core::getAttrib(x, crate::attrib_core::R_NamesSymbol());
    let nyn = crate::attrib_core::getAttrib(y, crate::attrib_core::R_NamesSymbol());
    let has_x = !nxn.is_null() && nxn != R_NilValue();
    let has_y = !nyn.is_null() && nyn != R_NilValue();
    if !has_x && !has_y {
        return;
    }
    let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    let _dn = protect(dn);
    SET_VECTOR_ELT(dn, 0, if has_x { nxn } else { R_NilValue() });
    SET_VECTOR_ELT(dn, 1, if has_y { nyn } else { R_NilValue() });
    crate::sexp::attrib_core::setAttrib(robj, crate::attrib_core::R_DimNamesSymbol(), dn);
}

pub(super) fn resolve_outer_fun(fun_arg: SEXP, rho: SEXP) -> SEXP {
    if fun_arg.is_null() || fun_arg == R_NilValue() {
        return find_fun(install_str("*"), rho);
    }
    let ty = TYPEOF(fun_arg);
    if ty == SEXPTYPE::CLOSXP || ty == SEXPTYPE::BUILTINSXP || ty == SEXPTYPE::SPECIALSXP {
        return fun_arg;
    }
    let sym = if ty == SEXPTYPE::STRSXP {
        let name = elt_to_string(fun_arg, 0);
        let cstr = CString::new(name.as_str()).unwrap_or_default();
        install_cstr(&cstr)
    } else if ty == SEXPTYPE::SYMSXP {
        fun_arg
    } else {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "object of mode 'function' was not found",
        );
    };
    let fun = find_fun(sym, rho);
    if fun.is_null() || fun == crate::sexp::globals::R_UnboundValue() {
        let name = if TYPEOF(sym) == SEXPTYPE::SYMSXP {
            let p = PRINTNAME(sym);
            if p.is_null() {
                "FUN".to_string()
            } else {
                charsxp_text(p).unwrap_or_default()
            }
        } else {
            "FUN".to_string()
        };
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            &format!("object '{name}' of mode 'function' was not found"),
        );
    }
    fun
}

pub(super) fn rep_times(x: SEXP, times: i64) -> SEXP {
    let n = XLENGTH(x);
    let out_n = n.saturating_mul(times);
    let result = Rf_allocVector3(TYPEOF(x), out_n);
    let _r = protect(result);
    for t in 0..times {
        for i in 0..n {
            copy_elt(x, i, result, t * n + i);
        }
    }
    result
}

pub(super) fn rep_each(x: SEXP, each: i64) -> SEXP {
    let n = XLENGTH(x);
    let out_n = n.saturating_mul(each);
    let result = Rf_allocVector3(TYPEOF(x), out_n);
    let _r = protect(result);
    for i in 0..n {
        for e in 0..each {
            copy_elt(x, i, result, i * each + e);
        }
    }
    result
}

pub(super) fn copy_elt(src: SEXP, si: i64, dst: SEXP, di: i64) {
    
        match TYPEOF(src) {
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                set_integer_checked(dst, (di as usize) as R_xlen_t, integer_elt_checked(src, (si as usize) as R_xlen_t));
            }
            t if t == SEXPTYPE::REALSXP => {
                set_real_checked(dst, (di as usize) as R_xlen_t, real_elt_checked(src, (si as usize) as R_xlen_t));
            }
            t if t == SEXPTYPE::CPLXSXP => {
                set_complex_checked(dst, (di as usize) as R_xlen_t, complex_elt_checked(src, (si as usize) as R_xlen_t));
            }
            t if t == SEXPTYPE::RAWSXP => {
                set_raw_checked(dst, (di as usize) as R_xlen_t, raw_elt_checked(src, (si as usize) as R_xlen_t));
            }
            t if t == SEXPTYPE::STRSXP => {
                SET_STRING_ELT(dst, di, STRING_ELT(src, si));
            }
            _ => {
                let v = extract_element(src, si);
                SET_VECTOR_ELT(dst, di, v);
            }
        }
    
}
