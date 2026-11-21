use super::*;
use crate::sexp::object::SexpMut;

/// R's `setdiff(x, y)` — elements in x but not in y.
pub fn do_setdiff(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = arg_by_name_or_position(args, &["x"], 0);
    let y = arg_by_name_or_position(args, &["y"], 1);
    if x.is_null() || x == R_NilValue() || TYPEOF(x) == SEXPTYPE::NILSXP {
        return R_NilValue();
    }

    let xn = XLENGTH(x);
    let yn = if y.is_null() || y == R_NilValue() {
        0
    } else {
        XLENGTH(y)
    };
    let t = TYPEOF(x);
    let owner = active_owner();
    if t == SEXPTYPE::VECSXP {
        let key_of = |obj: SEXP, i: R_xlen_t| {
            let obj_owned = owner_sexp(owner, obj);
            let elt = obj_owned
                .try_vector_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            if elt.as_raw().is_null() || elt.as_raw() == R_NilValue() {
                "NULL".to_string()
            } else {
                elt_to_string(elt.as_raw(), 0)
            }
        };
        let mut y_keys = std::collections::BTreeSet::new();
        for i in 0..yn {
            y_keys.insert(key_of(y, i));
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut result_indices = Vec::new();
        for i in 0..xn {
            let key = key_of(x, i);
            if !y_keys.contains(&key) && seen.insert(key) {
                result_indices.push(i);
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, result_indices.len() as R_xlen_t);
        let _g = protect(result);
        let x_owned = owner_sexp(owner, x);
        let mut result_mut = owner
            .sexp(result)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        for (out, &src) in result_indices.iter().enumerate() {
            let elt = x_owned
                .try_vector_elt(src)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            result_mut
                .try_set_vector_elt(out as R_xlen_t, elt)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        return result;
    }
    let sexptype = SEXPTYPE(t);
    let mut y_keys: std::collections::BTreeSet<AtomicUniqueKey> = std::collections::BTreeSet::new();
    for i in 0..yn {
        y_keys.insert(atomic_unique_key(y, i, sexptype));
    }
    let mut result_indices: Vec<R_xlen_t> = Vec::new();
    let mut seen: std::collections::BTreeSet<AtomicUniqueKey> = std::collections::BTreeSet::new();
    for i in 0..xn {
        let key = atomic_unique_key(x, i, sexptype);
        if !y_keys.contains(&key) && seen.insert(key) {
            result_indices.push(i);
        }
    }
    let result = Rf_allocVector3(t, result_indices.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    for (out, &src) in result_indices.iter().enumerate() {
        copy_atomic_element(result, out as R_xlen_t, x, src, sexptype);
    }
    result
}

/// R's `union(x, y)` — unique elements from both vectors.
pub fn do_union(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = arg_by_name_or_position(args, &["x"], 0);
    let y = arg_by_name_or_position(args, &["y"], 1);
    let t = if !x.is_null() && x != R_NilValue() {
        TYPEOF(x)
    } else if !y.is_null() && y != R_NilValue() {
        TYPEOF(y)
    } else {
        SEXPTYPE::INTSXP.as_c_int()
    };
    let sexptype = SEXPTYPE(t);
    let mut seen: std::collections::BTreeSet<AtomicUniqueKey> = std::collections::BTreeSet::new();
    let mut result_sources: Vec<(SEXP, R_xlen_t)> = Vec::new();
    let mut add_from = |src: SEXP| {
        if !src.is_null() && src != R_NilValue() {
            let n = XLENGTH(src);
            for i in 0..n {
                let key = atomic_unique_key(src, i, sexptype);
                if seen.insert(key) {
                    result_sources.push((src, i));
                }
            }
        }
    };
    add_from(x);
    add_from(y);
    let result = Rf_allocVector3(t, result_sources.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    for (out, &(src, src_index)) in result_sources.iter().enumerate() {
        copy_atomic_element(result, out as R_xlen_t, src, src_index, sexptype);
    }
    result
}

/// R's `intersect(x, y)` — elements common to both vectors.
pub fn do_intersect(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = arg_by_name_or_position(args, &["x"], 0);
    let y = arg_by_name_or_position(args, &["y"], 1);
    if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
        return Rf_allocVector3(TYPEOF(x), 0);
    }
    let t = TYPEOF(x);
    let sexptype = SEXPTYPE(t);
    let xn = XLENGTH(x);
    let yn = XLENGTH(y);
    let mut y_keys: std::collections::BTreeSet<AtomicUniqueKey> = std::collections::BTreeSet::new();
    for i in 0..yn {
        y_keys.insert(atomic_unique_key(y, i, sexptype));
    }
    let mut seen: std::collections::BTreeSet<AtomicUniqueKey> = std::collections::BTreeSet::new();
    let mut result_indices: Vec<R_xlen_t> = Vec::new();
    for i in 0..xn {
        let key = atomic_unique_key(x, i, sexptype);
        if y_keys.contains(&key) && seen.insert(key) {
            result_indices.push(i);
        }
    }
    let result = Rf_allocVector3(t, result_indices.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    for (out, &src) in result_indices.iter().enumerate() {
        copy_atomic_element(result, out as R_xlen_t, x, src, sexptype);
    }
    result
}

/// R's `setequal(x, y)` — TRUE if x and y contain the same unique values.
pub fn do_setequal(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = arg_by_name_or_position(args, &["x"], 0);
    let y = arg_by_name_or_position(args, &["y"], 1);
    if (x.is_null() || x == R_NilValue()) && (y.is_null() || y == R_NilValue()) {
        return Rf_ScalarLogical(TRUE);
    }
    if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let xn = XLENGTH(x);
    let yn = XLENGTH(y);
    let tx = TYPEOF(x);
    let sexptype = SEXPTYPE(tx);
    let mut x_set: std::collections::BTreeSet<AtomicUniqueKey> = std::collections::BTreeSet::new();
    let mut y_set: std::collections::BTreeSet<AtomicUniqueKey> = std::collections::BTreeSet::new();
    for i in 0..xn {
        x_set.insert(atomic_unique_key(x, i, sexptype));
    }
    for i in 0..yn {
        y_set.insert(atomic_unique_key(y, i, sexptype));
    }
    Rf_ScalarLogical(if x_set == y_set { TRUE } else { FALSE })
}

// ---------------------------------------------------------------------------
// do_order — order indices for sorting
// ---------------------------------------------------------------------------

/// GNU `.Internal(order(na.last, decreasing, ...))`.
pub fn do_order(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    if args.is_null() || args == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }
    let nalast = CAR(args);
    let rest = CDR(args);
    let decreasing_s = if rest.is_null() || rest == R_NilValue() {
        R_NilValue()
    } else {
        CAR(rest)
    };
    let keys = if rest.is_null() || rest == R_NilValue() {
        R_NilValue()
    } else {
        CDR(rest)
    };
    if keys.is_null() || keys == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }
    let decreasing = crate::mainutils::coerce::asLogical(decreasing_s) == TRUE;
    let na_placement = {
        let raw = crate::mainutils::coerce::asLogical(nalast);
        match raw {
            NA_LOGICAL => SortNaPlacement::Remove,
            FALSE => SortNaPlacement::First,
            _ => SortNaPlacement::Last,
        }
    };
    let mut key_vecs = Vec::new();
    let mut ap = keys;
    while !ap.is_null() && ap != R_NilValue() {
        key_vecs.push(CAR(ap));
        ap = CDR(ap);
    }
    if key_vecs.is_empty() || key_vecs[0].is_null() || key_vecs[0] == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }
    let key_type = TYPEOF(key_vecs[0]);
    if key_type != SEXPTYPE::LGLSXP
        && key_type != SEXPTYPE::INTSXP
        && key_type != SEXPTYPE::REALSXP
        && key_type != SEXPTYPE::CPLXSXP
        && key_type != SEXPTYPE::STRSXP
        && key_type != SEXPTYPE::RAWSXP
    {
        base_error("unimplemented type in 'order'".to_string());
    }

    let ordered_indices = if key_vecs.len() == 1 {
        ordered_atomic_indices(key_vecs[0], decreasing, na_placement)
    } else {
        multi_key_indices(&key_vecs, decreasing, na_placement)
    };
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, ordered_indices.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    let owner = active_owner();
    let mut dst = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for (i, &orig_idx) in ordered_indices.iter().enumerate() {
        dst.try_set_integer_elt(i as R_xlen_t, (orig_idx + 1) as c_int)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    result
}

fn multi_key_indices(
    keys: &[SEXP],
    decreasing: bool,
    na_placement: SortNaPlacement,
) -> Vec<R_xlen_t> {
    {
        let n = XLENGTH(keys[0]);
        for key in keys.iter().skip(1) {
            if XLENGTH(*key) != n {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "argument lengths differ".to_string(),
                });
            }
        }
        let na_last = !matches!(na_placement, SortNaPlacement::First);
        let mut present = Vec::new();
        let mut missing = Vec::new();
        for i in 0..n as usize {
            let any_na = keys.iter().any(|key| order_key_is_na(*key, i));
            if any_na && matches!(na_placement, SortNaPlacement::Remove) {
                continue;
            }
            if any_na {
                missing.push(i as R_xlen_t);
            }
            present.push(i as R_xlen_t);
        }
        // na.last = NA drops incomplete rows. Otherwise NA is only special
        // on the key being compared, so every row stays in the ordering.
        let mut rows = if matches!(na_placement, SortNaPlacement::Remove) {
            present
        } else {
            let mut all = Vec::with_capacity(n as usize);
            all.extend(missing.iter().copied());
            // missing was recorded but we want every index, once.
            all.clear();
            all.extend(0..n);
            all
        };
        rows.sort_by(|&a, &b| {
            for key in keys {
                let a_na = order_key_is_na(*key, a as usize);
                let b_na = order_key_is_na(*key, b as usize);
                if a_na || b_na {
                    if a_na && b_na {
                        continue;
                    }
                    return if a_na == na_last {
                        std::cmp::Ordering::Greater
                    } else {
                        std::cmp::Ordering::Less
                    };
                }
                let mut ordering = order_key_cmp(*key, a as usize, b as usize);
                if decreasing {
                    ordering = ordering.reverse();
                }
                if ordering != std::cmp::Ordering::Equal {
                    return ordering;
                }
            }
            std::cmp::Ordering::Equal
        });
        rows
    }
}

fn order_key_is_na(key: SEXP, i: usize) -> bool {
    let t = TYPEOF(key);
    let owner = active_owner();
    let key_owned = owner_sexp(owner, key);
    let i = i as R_xlen_t;
    match t {
        t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
            integer_like_elt(&key_owned, i) == NA_INTEGER
        }
        t if t == SEXPTYPE::REALSXP => ISNAN(
            key_owned
                .try_real_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
        ),
        t if t == SEXPTYPE::STRSXP => charsxp_is_na(
            key_owned
                .try_string_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                .as_raw(),
        ),
        t if t == SEXPTYPE::RAWSXP => false,
        _ => ISNAN(elt_real_safe(key, i)),
    }
}

fn order_key_cmp(key: SEXP, i: usize, j: usize) -> std::cmp::Ordering {
    let t = TYPEOF(key);
    let owner = active_owner();
    let key_owned = owner_sexp(owner, key);
    let (i, j) = (i as R_xlen_t, j as R_xlen_t);
    match t {
        t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
            integer_like_elt(&key_owned, i).cmp(&integer_like_elt(&key_owned, j))
        }
        t if t == SEXPTYPE::REALSXP => {
            let a = key_owned
                .try_real_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let b = key_owned
                .try_real_elt(j)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
        }
        t if t == SEXPTYPE::STRSXP => {
            let a = key_owned
                .try_string_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let b = key_owned
                .try_string_elt(j)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            compare_charsxp_for_sort(a.as_raw(), b.as_raw())
        }
        t if t == SEXPTYPE::RAWSXP => {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "unimplemented type 'raw' in 'listgreater'\n".to_string(),
            });
        }
        _ => elt_real_safe(key, i)
            .partial_cmp(&elt_real_safe(key, j))
            .unwrap_or(std::cmp::Ordering::Equal),
    }
}

pub(crate) fn reject_unorderable(x: SEXP) {
    let t = TYPEOF(x);
    if t == SEXPTYPE::VECSXP || t == SEXPTYPE::LISTSXP {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "unimplemented type in 'order'".to_string(),
        });
    }
}

pub(crate) fn ordered_atomic_indices(
    x: SEXP,
    decreasing: bool,
    na_placement: SortNaPlacement,
) -> Vec<R_xlen_t> {
    if TYPEOF(x) == SEXPTYPE::RAWSXP {
        base_error("unimplemented type 'raw' in 'orderVector1'\n".to_string());
    }
    reject_unorderable(x);
    let n = XLENGTH(x);
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    let mut missing_indices: Vec<R_xlen_t> = Vec::new();
    let mut ordered_indices: Vec<R_xlen_t> = match TYPEOF(x) {
        t if t == SEXPTYPE::STRSXP => {
            let mut values: Vec<(SEXP, R_xlen_t)> = Vec::with_capacity(n as usize);
            for i in 0..n {
                let value = x_owned
                    .try_string_elt(i)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    .as_raw();
                if charsxp_is_na(value) {
                    missing_indices.push(i);
                } else {
                    values.push((value, i));
                }
            }
            values.sort_by(|a, b| {
                let ordering = compare_charsxp_for_sort(a.0, b.0);
                if decreasing {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
            values.into_iter().map(|(_, index)| index).collect()
        }
        t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
            let mut values: Vec<(c_int, R_xlen_t)> = Vec::with_capacity(n as usize);
            for i in 0..n {
                let value = integer_like_elt(&x_owned, i);
                if value == NA_INTEGER {
                    missing_indices.push(i);
                } else {
                    values.push((value, i));
                }
            }
            values.sort_by(|a, b| {
                let ordering = a.0.cmp(&b.0);
                if decreasing {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
            values.into_iter().map(|(_, index)| index).collect()
        }
        t if t == SEXPTYPE::REALSXP => {
            let mut values: Vec<(f64, R_xlen_t)> = Vec::with_capacity(n as usize);
            for i in 0..n {
                let value = x_owned
                    .try_real_elt(i)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                if ISNAN(value) {
                    missing_indices.push(i);
                } else {
                    values.push((value, i));
                }
            }
            values.sort_by(|a, b| {
                let ordering = a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal);
                if decreasing {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
            values.into_iter().map(|(_, index)| index).collect()
        }
        t if t == SEXPTYPE::RAWSXP => {
            let mut values: Vec<(u8, R_xlen_t)> = Vec::with_capacity(n as usize);
            for i in 0..n {
                values.push((
                    x_owned
                        .try_raw_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    i,
                ));
            }
            values.sort_by(|a, b| {
                let ordering = a.0.cmp(&b.0);
                if decreasing {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
            values.into_iter().map(|(_, index)| index).collect()
        }
        _ => {
            let mut values: Vec<(f64, R_xlen_t)> = Vec::with_capacity(n as usize);
            for i in 0..n {
                let value = elt_real_safe(x, i);
                if ISNAN(value) {
                    missing_indices.push(i);
                } else {
                    values.push((value, i));
                }
            }
            values.sort_by(|a, b| {
                let ordering = a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal);
                if decreasing {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
            values.into_iter().map(|(_, index)| index).collect()
        }
    };

    match na_placement {
        SortNaPlacement::First => {
            let mut with_missing = missing_indices;
            with_missing.extend(ordered_indices);
            with_missing
        }
        SortNaPlacement::Last | SortNaPlacement::Keep => {
            ordered_indices.extend(missing_indices);
            ordered_indices
        }
        SortNaPlacement::Remove => ordered_indices,
    }
}

pub(crate) fn order_na_placement(args: SEXP, position: usize) -> SortNaPlacement {
    let arg = arg_by_name_or_position(args, &["na.last"], position);
    if arg.is_null() || arg == R_NilValue() || XLENGTH(arg) == 0 {
        return SortNaPlacement::Last;
    }
    let owner = active_owner();
    let arg_owned = owner_sexp(owner, arg);
    if TYPEOF(arg) == SEXPTYPE::STRSXP {
        let s = arg_owned
            .try_string_elt(0)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if !s.as_raw().is_null() {
            let is_keep = s
                .header()
                .char_bytes()
                .map(|bytes| bytes == b"keep")
                .unwrap_or(false);
            if is_keep {
                return SortNaPlacement::Keep;
            }
        }
        return SortNaPlacement::Last;
    }
    let raw = if TYPEOF(arg) == SEXPTYPE::LGLSXP || TYPEOF(arg) == SEXPTYPE::INTSXP {
        integer_like_elt(&arg_owned, 0)
    } else if TYPEOF(arg) == SEXPTYPE::REALSXP {
        let value = arg_owned
            .try_real_elt(0)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if ISNAN(value) {
            NA_LOGICAL
        } else {
            value as c_int
        }
    } else {
        TRUE
    };
    match raw {
        NA_LOGICAL => SortNaPlacement::Remove,
        FALSE => SortNaPlacement::First,
        _ => SortNaPlacement::Last,
    }
}

// ---------------------------------------------------------------------------
// do_rank — ranks of elements
// ---------------------------------------------------------------------------

/// R's `rank(x)` — returns ranks of elements (average ties method).
pub fn do_rank(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = arg_by_name_or_position(args, &["x"], 0);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
    }
    if TYPEOF(x) == SEXPTYPE::RAWSXP {
        base_error("raw vectors cannot be sorted".to_string());
    }
    reject_unorderable(x);
    let n = XLENGTH(x);
    let na_placement = order_na_placement(args, 1);
    let ties_method = rank_ties_method(args);
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    let mut missing_indices: Vec<R_xlen_t> = Vec::new();
    let mut ranks = vec![NA_REAL; n as usize];

    match TYPEOF(x) {
        t if t == SEXPTYPE::STRSXP => {
            let mut values: Vec<(SEXP, R_xlen_t)> = Vec::with_capacity(n as usize);
            for i in 0..n {
                let value = x_owned
                    .try_string_elt(i)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    .as_raw();
                if charsxp_is_na(value) {
                    missing_indices.push(i);
                } else {
                    values.push((value, i));
                }
            }
            values.sort_by(|a, b| compare_charsxp_for_sort(a.0, b.0));
            assign_tied_ranks(&mut ranks, &values, ties_method, 0, |a, b| {
                compare_charsxp_for_sort(a.0, b.0) == std::cmp::Ordering::Equal
            });
        }
        t if t == SEXPTYPE::RAWSXP => {
            let mut values: Vec<(u8, R_xlen_t)> = Vec::with_capacity(n as usize);
            for i in 0..n {
                values.push((
                    x_owned
                        .try_raw_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    i,
                ));
            }
            values.sort_by(|a, b| a.0.cmp(&b.0));
            assign_tied_ranks(&mut ranks, &values, ties_method, 0, |a, b| a.0 == b.0);
        }
        t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
            let mut values: Vec<(c_int, R_xlen_t)> = Vec::with_capacity(n as usize);
            for i in 0..n {
                let value = integer_like_elt(&x_owned, i);
                if value == NA_INTEGER {
                    missing_indices.push(i);
                } else {
                    values.push((value, i));
                }
            }
            values.sort_by(|a, b| a.0.cmp(&b.0));
            assign_tied_ranks(&mut ranks, &values, ties_method, 0, |a, b| a.0 == b.0);
        }
        _ => {
            let mut values: Vec<(f64, R_xlen_t)> = Vec::with_capacity(n as usize);
            for i in 0..n {
                let value = elt_real_safe(x, i);
                if ISNAN(value) {
                    missing_indices.push(i);
                } else {
                    values.push((value, i));
                }
            }
            values.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            assign_tied_ranks(&mut ranks, &values, ties_method, 0, |a, b| a.0 == b.0);
        }
    }

    let nonmissing_count = n as usize - missing_indices.len();
    let mut is_missing = vec![false; n as usize];
    for &index in &missing_indices {
        is_missing[index as usize] = true;
    }
    match na_placement {
        SortNaPlacement::First => {
            for (i, rank) in ranks.iter_mut().enumerate() {
                if !is_missing[i] {
                    *rank += missing_indices.len() as f64;
                }
            }
            for (offset, &index) in missing_indices.iter().enumerate() {
                ranks[index as usize] = (offset + 1) as f64;
            }
        }
        SortNaPlacement::Last => {
            for (offset, &index) in missing_indices.iter().enumerate() {
                ranks[index as usize] = (nonmissing_count + offset + 1) as f64;
            }
        }
        SortNaPlacement::Remove | SortNaPlacement::Keep => {}
    }

    let output_len = if na_placement == SortNaPlacement::Remove {
        nonmissing_count
    } else {
        n as usize
    };
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, output_len as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    let mut dst = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let mut out = 0usize;
    for i in 0..n as usize {
        if na_placement == SortNaPlacement::Remove && is_missing[i] {
            continue;
        }
        dst.try_set_real_elt(out as R_xlen_t, ranks[i])
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        out += 1;
    }
    let names = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
    if !names.is_null()
        && names != R_NilValue()
        && TYPEOF(names) == SEXPTYPE::STRSXP
        && XLENGTH(names) == n
    {
        let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, output_len as R_xlen_t);
        let _on = protect(out_names);
        let names_owned = owner_sexp(owner, names);
        let mut names_mut = owner
            .sexp(out_names)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut out = 0i64;
        for i in 0..n {
            if na_placement == SortNaPlacement::Remove && is_missing[i as usize] {
                continue;
            }
            let elt = names_owned
                .try_string_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            names_mut
                .try_set_string_elt(out, elt)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            out += 1;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            out_names,
        );
    }
    result
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RankTiesMethod {
    Average,
    First,
    Last,
    Min,
    Max,
}

fn rank_ties_method(args: SEXP) -> RankTiesMethod {
    {
        let arg = arg_by_name_or_position(args, &["ties.method"], 2);
        if arg.is_null()
            || arg == R_NilValue()
            || TYPEOF(arg) != SEXPTYPE::STRSXP
            || XLENGTH(arg) == 0
        {
            return RankTiesMethod::Average;
        }
        match elt_to_string(arg, 0).as_str() {
            "first" => RankTiesMethod::First,
            "last" => RankTiesMethod::Last,
            "min" => RankTiesMethod::Min,
            "max" => RankTiesMethod::Max,
            _ => RankTiesMethod::Average,
        }
    }
}

fn assign_tied_ranks<T, F>(
    ranks: &mut [f64],
    values: &[(T, R_xlen_t)],
    ties_method: RankTiesMethod,
    rank_offset: usize,
    same_key: F,
) where
    F: Fn(&(T, R_xlen_t), &(T, R_xlen_t)) -> bool,
{
    let mut i = 0usize;
    while i < values.len() {
        let mut j = i + 1;
        while j < values.len() && same_key(&values[i], &values[j]) {
            j += 1;
        }
        match ties_method {
            RankTiesMethod::Average => {
                let avg_rank = (rank_offset + i + rank_offset + j + 1) as f64 / 2.0;
                for item in &values[i..j] {
                    ranks[item.1 as usize] = avg_rank;
                }
            }
            RankTiesMethod::First => {
                for (offset, item) in values[i..j].iter().enumerate() {
                    ranks[item.1 as usize] = (rank_offset + i + offset + 1) as f64;
                }
            }
            RankTiesMethod::Last => {
                for (offset, item) in values[i..j].iter().enumerate() {
                    ranks[item.1 as usize] = (rank_offset + j - offset) as f64;
                }
            }
            RankTiesMethod::Min => {
                let rank = (rank_offset + i + 1) as f64;
                for item in &values[i..j] {
                    ranks[item.1 as usize] = rank;
                }
            }
            RankTiesMethod::Max => {
                let rank = (rank_offset + j) as f64;
                for item in &values[i..j] {
                    ranks[item.1 as usize] = rank;
                }
            }
        }
        i = j;
    }
}

// ---------------------------------------------------------------------------
// do_duplicated — identify duplicates
// ---------------------------------------------------------------------------

/// R's `duplicated(x, incomparables, fromLast, nmax)` — returns logical vector, TRUE for duplicated elements.
///
/// - `incomparables`: values to exclude from duplicate checking (typically NA or FALSE)
/// - `fromLast`: if TRUE, consider last occurrence as original (mark earlier as dup)
/// - `nmax`: max number of unique elements expected (optimization hint; NA_INTEGER = no limit)
pub fn do_duplicated(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = arg_by_name_or_position(args, &["x"], 0);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
    }

    let incomparables = arg_by_name_or_position(args, &["incomparables"], 1);
    let from_last = logical_arg_by_name_or_position(args, "fromLast", 2).unwrap_or(false);
    let nmax = integer_arg_by_name_or_position(args, "nmax", 3).unwrap_or(NA_INTEGER);

    // Build incomparables set
    let mut incomparable_set: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    if !incomparables_are_none(incomparables) {
        let in_n = XLENGTH(incomparables);
        for i in 0..in_n {
            incomparable_set.insert(duplicated_key(incomparables, i, SEXPTYPE(TYPEOF(x))));
        }
    }

    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    let owner = active_owner();
    let mut dst = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let put = |dst: &mut SexpMut<'_>, i: R_xlen_t, value: c_int| {
        dst.try_set_logical_elt(i, value)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    };

    // Compute nmax limit
    let effective_nmax: usize = if nmax == NA_INTEGER || nmax <= 0 {
        usize::MAX
    } else {
        nmax as usize
    };

    if from_last {
        // Scan from last to first; last occurrence is original, earlier are duplicates
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for i in 0..n {
            put(&mut dst, i, FALSE);
        }
        for i in (0..n).rev() {
            let s = duplicated_key(x, i, SEXPTYPE(TYPEOF(x)));
            if incomparable_set.contains(&s) {
                put(&mut dst, i, FALSE);
            } else if seen.contains(&s) {
                put(&mut dst, i, TRUE);
            } else {
                seen.insert(s);
                put(&mut dst, i, FALSE);
                if seen.len() >= effective_nmax {
                    for j in 0..i {
                        let sj = duplicated_key(x, j, SEXPTYPE(TYPEOF(x)));
                        if !incomparable_set.contains(&sj) {
                            put(&mut dst, j, TRUE);
                        }
                    }
                    break;
                }
            }
        }
    } else {
        // Scan from first to last; first occurrence is original, later are duplicates
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for i in 0..n {
            let s = duplicated_key(x, i, SEXPTYPE(TYPEOF(x)));
            if incomparable_set.contains(&s) {
                put(&mut dst, i, FALSE);
            } else if seen.contains(&s) {
                put(&mut dst, i, TRUE);
            } else {
                seen.insert(s);
                put(&mut dst, i, FALSE);
                if seen.len() >= effective_nmax {
                    // Everything remaining is a duplicate
                    for j in (i + 1)..n {
                        let sj = duplicated_key(x, j, SEXPTYPE(TYPEOF(x)));
                        if incomparable_set.contains(&sj) {
                            put(&mut dst, j, FALSE);
                        } else {
                            put(&mut dst, j, TRUE);
                        }
                    }
                    break;
                }
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// do_anyDuplicated — check for any duplicates
// ---------------------------------------------------------------------------

/// R's `anyDuplicated(x, incomparables, fromLast, nmax)` — returns index of first duplicate (0 if none).
///
/// Supports incomparables, fromLast, and nmax parameters just like `duplicated()`.
pub fn do_anyDuplicated(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = arg_by_name_or_position(args, &["x"], 0);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarInteger(0);
    }

    let incomparables = arg_by_name_or_position(args, &["incomparables"], 1);
    let from_last = logical_arg_by_name_or_position(args, "fromLast", 2).unwrap_or(false);
    let nmax = integer_arg_by_name_or_position(args, "nmax", 3).unwrap_or(NA_INTEGER);

    // Build incomparables set
    let mut incomparable_set: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    if !incomparables_are_none(incomparables) {
        let in_n = XLENGTH(incomparables);
        for i in 0..in_n {
            incomparable_set.insert(duplicated_key(incomparables, i, SEXPTYPE(TYPEOF(x))));
        }
    }

    let n = XLENGTH(x);
    let effective_nmax: usize = if nmax == NA_INTEGER || nmax <= 0 {
        usize::MAX
    } else {
        nmax as usize
    };

    if from_last {
        // From last: find last duplicated element index
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for i in (0..n).rev() {
            let s = duplicated_key(x, i, SEXPTYPE(TYPEOF(x)));
            if !incomparable_set.contains(&s) {
                if seen.contains(&s) {
                    return Rf_ScalarInteger((i + 1) as c_int);
                } else {
                    seen.insert(s);
                    if seen.len() >= effective_nmax {
                        break;
                    }
                }
            }
        }
        Rf_ScalarInteger(0)
    } else {
        // From first: find first duplicated element index
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for i in 0..n {
            let s = duplicated_key(x, i, SEXPTYPE(TYPEOF(x)));
            if !incomparable_set.contains(&s) {
                if seen.contains(&s) {
                    return Rf_ScalarInteger((i + 1) as c_int);
                }
                seen.insert(s);
                if seen.len() >= effective_nmax {
                    break;
                }
            }
        }
        Rf_ScalarInteger(0)
    }
}

// ---------------------------------------------------------------------------
// do_duplicated.array — array deduplication along margins
// ---------------------------------------------------------------------------

/// R's `duplicated.array(x, MARGIN, fromLast)` — finds duplicated rows/columns in an array.
///
/// - `x`: array or matrix
/// - `MARGIN`: which margin to check (1=rows, 2=cols, etc.)
/// - `fromLast`: if TRUE, last occurrence is original
pub fn do_duplicated_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }

        // Primitive call supplies (x, MARGIN) or (x, MARGIN, fromLast).
        let margin = {
            let rest = CDR(args);
            if rest.is_null() || rest == R_NilValue() {
                1i32
            } else {
                real_or_default(CAR(rest), 1.0) as i32
            }
        };
        let from_last = {
            let rest = CDR(CDR(args));
            if rest.is_null() || rest == R_NilValue() {
                false
            } else {
                real_or_default(CAR(rest), 0.0) != 0.0
            }
        };

        let n = XLENGTH(x);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }

        // Get dimensions
        let dim = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dim".as_ptr()));

        if dim.is_null() || dim == R_NilValue() || XLENGTH(dim) < 2 {
            // Not really an array — fall back to regular duplicated
            let mut new_args = R_NilValue();
            // push nmax as NA
            new_args = Rf_cons(Rf_ScalarInteger(NA_INTEGER), new_args);
            new_args = Rf_cons(
                Rf_ScalarLogical(if from_last { TRUE } else { FALSE }),
                new_args,
            );
            new_args = Rf_cons(R_NilValue(), new_args); // incomparables
            new_args = Rf_cons(x, new_args);
            return do_duplicated(_call, _op, new_args, _rho);
        }

        let dims_len = XLENGTH(dim);
        let owner = active_owner();
        let dim_owned = owner_sexp(owner, dim);
        let dim_elt = |i: R_xlen_t| -> c_int {
            dim_owned
                .try_integer_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        };
        let nrows = dim_elt(0) as usize;
        let ncols = if dims_len >= 2 {
            dim_elt(1) as usize
        } else {
            1
        };

        if margin == 0 {
            let flat = crate::mainutils::duplicate::Rf_duplicate(x);
            let _f = protect(flat);
            crate::sexp::attrib_core::setAttrib(flat, Rf_install(c"dim".as_ptr()), R_NilValue());
            crate::sexp::attrib_core::setAttrib(
                flat,
                Rf_install(c"dimnames".as_ptr()),
                R_NilValue(),
            );

            let new_args = Rf_cons(flat, R_NilValue());
            let _n = protect(new_args);
            let result = do_duplicated(_call, _op, new_args, _rho);
            let attrs = crate::sexp::accessors::ATTRIB(x);
            if !attrs.is_null() && attrs != R_NilValue() {
                crate::sexp::accessors::SET_ATTRIB(
                    result,
                    crate::mainutils::duplicate::Rf_duplicate(attrs),
                );
            }
            return result;
        }

        // For 2D arrays, support MARGIN=1 (rows) and MARGIN=2 (columns)
        if margin == 1 && dims_len == 2 {
            // Duplicate rows
            let total = nrows;
            let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let mut dst = owner
                .sexp(result)
                .and_then(SexpMut::try_from_checked)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));

            // Hash each row as a string
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            let t = TYPEOF(x);

            if from_last {
                // First pass collect, second pass mark
                let mut row_strings: Vec<String> = Vec::with_capacity(total);
                for row in 0..total {
                    let mut parts: Vec<String> = Vec::with_capacity(ncols);
                    for col in 0..ncols {
                        let idx = row + col * nrows; // column-major
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    row_strings.push(parts.join("\x01"));
                }
                // Collect from end
                let mut unique_from_end: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                for row in (0..total).rev() {
                    unique_from_end.insert(row_strings[row].clone());
                }
                // Mark from start
                let mut encountered: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                for row in 0..total {
                    if encountered.contains(&row_strings[row]) {
                        dst.try_set_logical_elt(row as R_xlen_t, TRUE)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                    } else {
                        encountered.insert(row_strings[row].clone());
                        dst.try_set_logical_elt(row as R_xlen_t, FALSE)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                    }
                }
            } else {
                for row in 0..total {
                    let mut parts: Vec<String> = Vec::with_capacity(ncols);
                    for col in 0..ncols {
                        let idx = row + col * nrows; // column-major
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    let key = parts.join("\x01");
                    if seen.contains(&key) {
                        dst.try_set_logical_elt(row as R_xlen_t, TRUE)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                    } else {
                        seen.insert(key);
                        dst.try_set_logical_elt(row as R_xlen_t, FALSE)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                    }
                }
            }

            result
        } else if margin == 2 && dims_len == 2 {
            // Duplicate columns
            let total = ncols;
            let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let mut dst = owner
                .sexp(result)
                .and_then(SexpMut::try_from_checked)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));

            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

            if from_last {
                let mut col_strings: Vec<String> = Vec::with_capacity(total);
                for col in 0..total {
                    let mut parts: Vec<String> = Vec::with_capacity(nrows);
                    for row in 0..nrows {
                        let idx = row + col * nrows;
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    col_strings.push(parts.join("\x01"));
                }
                let mut encountered: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                for col in 0..total {
                    if encountered.contains(&col_strings[col]) {
                        dst.try_set_logical_elt(col as R_xlen_t, TRUE)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                    } else {
                        encountered.insert(col_strings[col].clone());
                        dst.try_set_logical_elt(col as R_xlen_t, FALSE)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                    }
                }
            } else {
                for col in 0..total {
                    let mut parts: Vec<String> = Vec::with_capacity(nrows);
                    for row in 0..nrows {
                        let idx = row + col * nrows;
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    let key = parts.join("\x01");
                    if seen.contains(&key) {
                        dst.try_set_logical_elt(col as R_xlen_t, TRUE)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                    } else {
                        seen.insert(key);
                        dst.try_set_logical_elt(col as R_xlen_t, FALSE)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                    }
                }
            }

            result
        } else {
            let m = (margin as usize).saturating_sub(1);
            if m >= dims_len as usize {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "MARGIN out of range".to_string(),
                });
            }
            let mut before: usize = 1;
            for i in 0..m {
                before *= dim_elt(i as R_xlen_t) as usize;
            }
            let n_slices = dim_elt(m as R_xlen_t) as usize;
            let mut after: usize = 1;
            for i in (m + 1)..dims_len as usize {
                after *= dim_elt(i as R_xlen_t) as usize;
            }
            let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n_slices as R_xlen_t);
            let mut dst = owner
                .sexp(result)
                .and_then(SexpMut::try_from_checked)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let mut keys: Vec<String> = Vec::with_capacity(n_slices);
            let stride = before * n_slices;
            for s in 0..n_slices {
                let mut parts = Vec::with_capacity(before * after);
                for b in 0..after {
                    for a in 0..before {
                        let idx = a + s * before + b * stride;
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                }
                keys.push(parts.join("\x01"));
            }
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            if from_last {
                for s in (0..n_slices).rev() {
                    if seen.contains(&keys[s]) {
                        dst.try_set_logical_elt(s as R_xlen_t, TRUE)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                    } else {
                        seen.insert(keys[s].clone());
                        dst.try_set_logical_elt(s as R_xlen_t, FALSE)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                    }
                }
            } else {
                for s in 0..n_slices {
                    if seen.contains(&keys[s]) {
                        dst.try_set_logical_elt(s as R_xlen_t, TRUE)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                    } else {
                        seen.insert(keys[s].clone());
                        dst.try_set_logical_elt(s as R_xlen_t, FALSE)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                    }
                }
            }
            result
        }
    }
}

// ---------------------------------------------------------------------------
// do_anyDuplicated.array — check for any duplicates in array along margin
// ---------------------------------------------------------------------------

/// R's `anyDuplicated.array(x, MARGIN, fromLast)` — returns index of first duplicate in array (0 if none).
pub fn do_anyDuplicated_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }

        // Parse MARGIN (default = 1)
        let margin = {
            let rest = CDR(args);
            if rest.is_null() || rest == R_NilValue() {
                1i32
            } else {
                real_or_default(CAR(rest), 1.0) as i32
            }
        };

        // Parse fromLast (default = FALSE)
        let from_last = {
            let rest = CDR(args);
            if rest.is_null() || rest == R_NilValue() {
                false
            } else {
                let rest2 = CDR(rest);
                if rest2.is_null() || rest2 == R_NilValue() {
                    false
                } else {
                    let v = real_or_default(CAR(rest2), 0.0);
                    v != 0.0
                }
            }
        };

        // Get dimensions
        let dim = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dim".as_ptr()));

        if dim.is_null() || dim == R_NilValue() || XLENGTH(dim) < 2 {
            // Not really an array — fall back to regular anyDuplicated
            let mut new_args = R_NilValue();
            new_args = Rf_cons(Rf_ScalarInteger(NA_INTEGER), new_args);
            new_args = Rf_cons(
                Rf_ScalarLogical(if from_last { TRUE } else { FALSE }),
                new_args,
            );
            new_args = Rf_cons(R_NilValue(), new_args);
            new_args = Rf_cons(x, new_args);
            return do_anyDuplicated(_call, _op, new_args, _rho);
        }

        let dims_len = XLENGTH(dim);
        let owner = active_owner();
        let dim_owned = owner_sexp(owner, dim);
        let dim_elt = |i: R_xlen_t| -> c_int {
            dim_owned
                .try_integer_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        };
        let nrows = dim_elt(0) as usize;
        let ncols = if dims_len >= 2 {
            dim_elt(1) as usize
        } else {
            1
        };

        if margin == 1 && dims_len == 2 {
            // Check duplicate rows
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            if from_last {
                let mut row_strings: Vec<String> = Vec::with_capacity(nrows);
                for row in 0..nrows {
                    let mut parts: Vec<String> = Vec::with_capacity(ncols);
                    for col in 0..ncols {
                        let idx = row + col * nrows;
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    row_strings.push(parts.join("\x01"));
                }
                let mut result_idx = 0i32;
                let mut encountered: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                for row in (0..nrows).rev() {
                    if encountered.contains(&row_strings[row]) {
                        result_idx = (row + 1) as c_int; // R 1-indexed
                    } else {
                        encountered.insert(row_strings[row].clone());
                    }
                }
                Rf_ScalarInteger(result_idx)
            } else {
                for row in 0..nrows {
                    let mut parts: Vec<String> = Vec::with_capacity(ncols);
                    for col in 0..ncols {
                        let idx = row + col * nrows;
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    let key = parts.join("\x01");
                    if seen.contains(&key) {
                        return Rf_ScalarInteger((row + 1) as c_int);
                    }
                    seen.insert(key);
                }
                Rf_ScalarInteger(0)
            }
        } else if margin == 2 && dims_len == 2 {
            // Check duplicate columns
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            if from_last {
                let mut col_strings: Vec<String> = Vec::with_capacity(ncols);
                for col in 0..ncols {
                    let mut parts: Vec<String> = Vec::with_capacity(nrows);
                    for row in 0..nrows {
                        let idx = row + col * nrows;
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    col_strings.push(parts.join("\x01"));
                }
                let mut result_idx = 0i32;
                let mut encountered: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                for col in (0..ncols).rev() {
                    if encountered.contains(&col_strings[col]) {
                        result_idx = (col + 1) as c_int;
                    } else {
                        encountered.insert(col_strings[col].clone());
                    }
                }
                Rf_ScalarInteger(result_idx)
            } else {
                for col in 0..ncols {
                    let mut parts: Vec<String> = Vec::with_capacity(nrows);
                    for row in 0..nrows {
                        let idx = row + col * nrows;
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    let key = parts.join("\x01");
                    if seen.contains(&key) {
                        return Rf_ScalarInteger((col + 1) as c_int);
                    }
                    seen.insert(key);
                }
                Rf_ScalarInteger(0)
            }
        } else {
            // Generic fallback
            let mut new_args = R_NilValue();
            new_args = Rf_cons(Rf_ScalarInteger(NA_INTEGER), new_args);
            new_args = Rf_cons(
                Rf_ScalarLogical(if from_last { TRUE } else { FALSE }),
                new_args,
            );
            new_args = Rf_cons(R_NilValue(), new_args);
            new_args = Rf_cons(x, new_args);
            do_anyDuplicated(_call, _op, new_args, _rho)
        }
    }
}

// ---------------------------------------------------------------------------
// do_match — match values in table
// ---------------------------------------------------------------------------

/// R's `match(x, table, nomatch, incomparables)` — first table index for each x.
pub fn do_match(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = match_arg(args, 0, "x", R_NilValue());
    let table = match_arg(args, 1, "table", R_NilValue());
    let nomatch_arg = match_arg(args, 2, "nomatch", Rf_ScalarInteger(NA_INTEGER));
    let incomparables = match_arg(args, 3, "incomparables", R_NilValue());
    let nomatch = integer_scalar_or(nomatch_arg, NA_INTEGER);

    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }
    let x_lt = crate::mainutils::essentials::sexp_has_class(x, "POSIXlt");
    let table_lt = crate::mainutils::essentials::sexp_has_class(table, "POSIXlt");
    let owner = active_owner();
    let n = if x_lt && TYPEOF(x) == SEXPTYPE::VECSXP && XLENGTH(x) > 0 {
        let first = owner_sexp(owner, x)
            .try_vector_elt(0)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        XLENGTH(first.as_raw())
    } else {
        XLENGTH(x)
    };
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    let mut dst = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));

    let common_type = match_common_type(x, table);
    let mut incomparable_set = BTreeSet::new();
    if !incomparables.is_null() && incomparables != R_NilValue() {
        for i in 0..XLENGTH(incomparables) {
            incomparable_set.insert(match_key(incomparables, i, common_type));
        }
    }

    let mut lookup: BTreeMap<MatchKey, c_int> = BTreeMap::new();
    if !table.is_null() && table != R_NilValue() {
        let tn = if table_lt && TYPEOF(table) == SEXPTYPE::VECSXP && XLENGTH(table) > 0 {
            let first = owner_sexp(owner, table)
                .try_vector_elt(0)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            XLENGTH(first.as_raw())
        } else {
            XLENGTH(table)
        };
        for i in 0..tn {
            let key = if table_lt {
                posixlt_key(table, i)
            } else {
                match_key(table, i, common_type)
            };
            lookup.entry(key).or_insert((i + 1) as c_int);
        }
    }
    for i in 0..n {
        let key = if x_lt {
            posixlt_key(x, i)
        } else {
            match_key(x, i, common_type)
        };
        let value = if incomparable_set.contains(&key) {
            nomatch
        } else {
            *lookup.get(&key).unwrap_or(&nomatch)
        };
        dst.try_set_integer_elt(i, value)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    result
}

fn posixlt_key(x: SEXP, index: R_xlen_t) -> MatchKey {
    let mut parts = String::new();
    let ncomp = XLENGTH(x);
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    for c in 0..ncomp {
        let col = x_owned
            .try_vector_elt(c)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            .as_raw();
        parts.push('\u{1f}');
        if col.is_null() || XLENGTH(col) == 0 {
            continue;
        }
        let i = index % XLENGTH(col);
        parts.push_str(&elt_to_string(col, i));
    }
    MatchKey::String(parts)
}

fn integer_scalar_or(arg: SEXP, default: c_int) -> c_int {
    if arg.is_null() || arg == R_NilValue() || arg == R_MissingArg() {
        return default;
    }
    let owner = active_owner();
    let arg_owned = owner_sexp(owner, arg);
    match SEXPTYPE(TYPEOF(arg)) {
        SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => {
            if XLENGTH(arg) < 1 {
                default
            } else {
                integer_like_elt(&arg_owned, 0)
            }
        }
        SEXPTYPE::REALSXP => {
            if XLENGTH(arg) < 1 {
                default
            } else {
                let value = arg_owned
                    .try_real_elt(0)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    default
                } else {
                    value as c_int
                }
            }
        }
        _ => default,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum MatchKey {
    Missing,
    String(String),
    Integer(c_int),
    Real(u64),
}

fn match_common_type(x: SEXP, table: SEXP) -> SEXPTYPE {
    {
        let xtype = if x.is_null() {
            SEXPTYPE::NILSXP
        } else {
            SEXPTYPE(TYPEOF(x))
        };
        let ttype = if table.is_null() || table == R_NilValue() {
            xtype
        } else {
            SEXPTYPE(TYPEOF(table))
        };
        if xtype == SEXPTYPE::STRSXP || ttype == SEXPTYPE::STRSXP {
            SEXPTYPE::STRSXP
        } else if xtype == SEXPTYPE::CPLXSXP || ttype == SEXPTYPE::CPLXSXP {
            SEXPTYPE::CPLXSXP
        } else if xtype == SEXPTYPE::REALSXP || ttype == SEXPTYPE::REALSXP {
            SEXPTYPE::REALSXP
        } else {
            SEXPTYPE::INTSXP
        }
    }
}

fn complex_match_key(x: SEXP, index: R_xlen_t) -> MatchKey {
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    let z = match TYPEOF(x) {
        t if t == SEXPTYPE::CPLXSXP => x_owned
            .try_complex_elt(index)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
        t if t == SEXPTYPE::REALSXP => {
            let r = x_owned
                .try_real_elt(index)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            crate::sexp::ffi::Rcomplex { r, i: 0.0 }
        }
        t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
            let v = integer_like_elt(&x_owned, index);
            let r = if v == NA_INTEGER { NA_REAL } else { v as f64 };
            crate::sexp::ffi::Rcomplex { r, i: 0.0 }
        }
        _ => return MatchKey::Missing,
    };
    if crate::sexp::ffi::R_IsNA(z.r) || crate::sexp::ffi::R_IsNA(z.i) {
        return MatchKey::String("\u{0}CNA".to_string());
    }
    let part = |v: f64| -> u64 {
        if ISNAN(v) {
            f64::NAN.to_bits()
        } else if v == 0.0 {
            0
        } else {
            v.to_bits()
        }
    };
    MatchKey::String(format!("\u{0}C{:x}:{:x}", part(z.r), part(z.i)))
}

fn match_key(x: SEXP, index: R_xlen_t, common_type: SEXPTYPE) -> MatchKey {
    if x.is_null() || x == R_NilValue() {
        return MatchKey::Missing;
    }
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    match common_type {
        SEXPTYPE::STRSXP => {
            let missing = match TYPEOF(x) {
                t if t == SEXPTYPE::STRSXP => {
                    x_owned
                        .try_string_elt(index)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                        .as_raw()
                        == crate::sexp::globals::R_NaString()
                }
                t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                    integer_like_elt(&x_owned, index) == NA_INTEGER
                }
                t if t == SEXPTYPE::REALSXP => {
                    x_owned
                        .try_real_elt(index)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                        .to_bits()
                        == crate::sexp::ffi::R_NA_BIT_PATTERN
                }
                _ => false,
            };
            if missing {
                MatchKey::Missing
            } else if sexp_has_class(x, "Date") && TYPEOF(x) == SEXPTYPE::REALSXP {
                let days = x_owned
                    .try_real_elt(index)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                MatchKey::String(
                    super::shared::date_days_to_iso(days).unwrap_or_else(|| "NA".to_string()),
                )
            } else {
                MatchKey::String(elt_to_string(x, index))
            }
        }
        SEXPTYPE::CPLXSXP => complex_match_key(x, index),
        SEXPTYPE::REALSXP => {
            let value = match TYPEOF(x) {
                t if t == SEXPTYPE::REALSXP => x_owned
                    .try_real_elt(index)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                    let value = integer_like_elt(&x_owned, index);
                    if value == NA_INTEGER {
                        NA_REAL
                    } else {
                        value as f64
                    }
                }
                _ => elt_to_string(x, index).parse::<f64>().unwrap_or(NA_REAL),
            };
            if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                MatchKey::Missing
            } else if value.is_nan() {
                MatchKey::Real(f64::NAN.to_bits())
            } else {
                MatchKey::Real(if value == 0.0 { 0u64 } else { value.to_bits() })
            }
        }
        _ => {
            let value = match TYPEOF(x) {
                t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                    integer_like_elt(&x_owned, index)
                }
                t if t == SEXPTYPE::REALSXP => {
                    let value = x_owned
                        .try_real_elt(index)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        NA_INTEGER
                    } else {
                        value as c_int
                    }
                }
                _ => elt_to_string(x, index)
                    .parse::<c_int>()
                    .unwrap_or(NA_INTEGER),
            };
            if value == NA_INTEGER {
                MatchKey::Missing
            } else {
                MatchKey::Integer(value)
            }
        }
    }
}
