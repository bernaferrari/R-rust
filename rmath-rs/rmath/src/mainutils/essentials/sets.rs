//! Essentials domain module `sets` — extracted verbatim from essentials.rs.

use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
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
use crate::sexp::ffi::{
    FALSE, ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Set operations: setdiff, union, intersect, setequal
// ---------------------------------------------------------------------------

/// R's `setdiff(x, y)` — elements in x but not in y.
pub unsafe fn do_setdiff(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        let y = arg_by_name_or_position(args, &["y"], 1);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(TYPEOF(x), 0);
        }
        let xn = XLENGTH(x);
        let yn = if y.is_null() || y == R_NilValue() {
            0
        } else {
            XLENGTH(y)
        };
        let t = TYPEOF(x);
        let sexptype = SEXPTYPE(t);
        let mut y_keys: std::collections::BTreeSet<AtomicUniqueKey> =
            std::collections::BTreeSet::new();
        for i in 0..yn {
            y_keys.insert(atomic_unique_key(y, i, sexptype));
        }
        let mut result_indices: Vec<R_xlen_t> = Vec::new();
        let mut seen: std::collections::BTreeSet<AtomicUniqueKey> =
            std::collections::BTreeSet::new();
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
}

/// R's `union(x, y)` — unique elements from both vectors.
pub unsafe fn do_union(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
        let mut seen: std::collections::BTreeSet<AtomicUniqueKey> =
            std::collections::BTreeSet::new();
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
}

/// R's `intersect(x, y)` — elements common to both vectors.
pub unsafe fn do_intersect(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        let y = arg_by_name_or_position(args, &["y"], 1);
        if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
            return Rf_allocVector3(TYPEOF(x), 0);
        }
        let t = TYPEOF(x);
        let sexptype = SEXPTYPE(t);
        let xn = XLENGTH(x);
        let yn = XLENGTH(y);
        let mut y_keys: std::collections::BTreeSet<AtomicUniqueKey> =
            std::collections::BTreeSet::new();
        for i in 0..yn {
            y_keys.insert(atomic_unique_key(y, i, sexptype));
        }
        let mut seen: std::collections::BTreeSet<AtomicUniqueKey> =
            std::collections::BTreeSet::new();
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
}

/// R's `setequal(x, y)` — TRUE if x and y contain the same unique values.
pub unsafe fn do_setequal(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
        let mut x_set: std::collections::BTreeSet<AtomicUniqueKey> =
            std::collections::BTreeSet::new();
        let mut y_set: std::collections::BTreeSet<AtomicUniqueKey> =
            std::collections::BTreeSet::new();
        for i in 0..xn {
            x_set.insert(atomic_unique_key(x, i, sexptype));
        }
        for i in 0..yn {
            y_set.insert(atomic_unique_key(y, i, sexptype));
        }
        Rf_ScalarLogical(if x_set == y_set { TRUE } else { FALSE })
    }
}

// ---------------------------------------------------------------------------
// do_order — order indices for sorting
// ---------------------------------------------------------------------------

/// R's `order(...)` — returns permutation of indices that sort the input.
pub unsafe fn do_order(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let n = XLENGTH(x);
        let decreasing = named_logical_arg(args, "decreasing").unwrap_or(false);
        let na_placement = order_na_placement(args, 1);
        let ordered_indices = ordered_atomic_indices(x, decreasing, na_placement);

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, ordered_indices.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);
        for (i, &orig_idx) in ordered_indices.iter().enumerate() {
            *dst.add(i) = (orig_idx + 1) as c_int;
        }
        result
    }
}

pub(crate) fn ordered_atomic_indices(
    x: SEXP,
    decreasing: bool,
    na_placement: SortNaPlacement,
) -> Vec<R_xlen_t> {
    unsafe {
        let n = XLENGTH(x);
        let mut missing_indices: Vec<R_xlen_t> = Vec::new();
        let mut ordered_indices: Vec<R_xlen_t> = match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP => {
                let mut values: Vec<(SEXP, R_xlen_t)> = Vec::with_capacity(n as usize);
                for i in 0..n {
                    let value = STRING_ELT(x, i);
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
                    let value = *INTEGER(x).add(i as usize);
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
                    let value = *REAL(x).add(i as usize);
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
            SortNaPlacement::Last => {
                ordered_indices.extend(missing_indices);
                ordered_indices
            }
            SortNaPlacement::Remove => ordered_indices,
        }
    }
}

pub(crate) fn order_na_placement(args: SEXP, position: usize) -> SortNaPlacement {
    unsafe {
        let arg = arg_by_name_or_position(args, &["na.last"], position);
        if arg.is_null() || arg == R_NilValue() || XLENGTH(arg) == 0 {
            return SortNaPlacement::Last;
        }
        let raw = if TYPEOF(arg) == SEXPTYPE::LGLSXP || TYPEOF(arg) == SEXPTYPE::INTSXP {
            *INTEGER(arg)
        } else if TYPEOF(arg) == SEXPTYPE::REALSXP {
            let value = *REAL(arg);
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
}

// ---------------------------------------------------------------------------
// do_rank — ranks of elements
// ---------------------------------------------------------------------------

/// R's `rank(x)` — returns ranks of elements (average ties method).
pub unsafe fn do_rank(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let n = XLENGTH(x);
        let na_placement = order_na_placement(args, 1);
        let ties_method = rank_ties_method(args);
        let mut missing_indices: Vec<R_xlen_t> = Vec::new();
        let mut ranks = vec![NA_REAL; n as usize];

        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP => {
                let mut values: Vec<(SEXP, R_xlen_t)> = Vec::with_capacity(n as usize);
                for i in 0..n {
                    let value = STRING_ELT(x, i);
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
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                let mut values: Vec<(c_int, R_xlen_t)> = Vec::with_capacity(n as usize);
                for i in 0..n {
                    let value = *INTEGER(x).add(i as usize);
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
            SortNaPlacement::Remove => {}
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
        let dst = REAL(result);
        let mut out = 0usize;
        for i in 0..n as usize {
            if na_placement == SortNaPlacement::Remove && is_missing[i] {
                continue;
            }
            *dst.add(out) = ranks[i];
            out += 1;
        }
        result
    }
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
    unsafe {
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
pub unsafe fn do_duplicated(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
        if !incomparables.is_null() && incomparables != R_NilValue() {
            let in_n = XLENGTH(incomparables);
            for i in 0..in_n {
                let s = elt_to_string(incomparables, i);
                incomparable_set.insert(s);
            }
        }

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);

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
                *dst.add(i as usize) = FALSE;
            }
            for i in (0..n).rev() {
                let s = elt_to_string(x, i);
                if incomparable_set.contains(&s) {
                    *dst.add(i as usize) = FALSE;
                } else if seen.contains(&s) {
                    *dst.add(i as usize) = TRUE;
                } else {
                    seen.insert(s);
                    *dst.add(i as usize) = FALSE;
                    if seen.len() >= effective_nmax {
                        for j in 0..i {
                            let sj = elt_to_string(x, j);
                            if !incomparable_set.contains(&sj) {
                                *dst.add(j as usize) = TRUE;
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
                let s = elt_to_string(x, i);
                if incomparable_set.contains(&s) {
                    *dst.add(i as usize) = FALSE;
                } else if seen.contains(&s) {
                    *dst.add(i as usize) = TRUE;
                } else {
                    seen.insert(s);
                    *dst.add(i as usize) = FALSE;
                    if seen.len() >= effective_nmax {
                        // Everything remaining is a duplicate
                        for j in (i + 1)..n {
                            let sj = elt_to_string(x, j);
                            if incomparable_set.contains(&sj) {
                                *dst.add(j as usize) = FALSE;
                            } else {
                                *dst.add(j as usize) = TRUE;
                            }
                        }
                        break;
                    }
                }
            }
        }

        result
    }
}

// ---------------------------------------------------------------------------
// do_anyDuplicated — check for any duplicates
// ---------------------------------------------------------------------------

/// R's `anyDuplicated(x, incomparables, fromLast, nmax)` — returns index of first duplicate (0 if none).
///
/// Supports incomparables, fromLast, and nmax parameters just like `duplicated()`.
pub unsafe fn do_anyDuplicated(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
        if !incomparables.is_null() && incomparables != R_NilValue() {
            let in_n = XLENGTH(incomparables);
            for i in 0..in_n {
                let s = elt_to_string(incomparables, i);
                incomparable_set.insert(s);
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
                let s = elt_to_string(x, i);
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
                let s = elt_to_string(x, i);
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
}

// ---------------------------------------------------------------------------
// do_duplicated.array — array deduplication along margins
// ---------------------------------------------------------------------------

/// R's `duplicated.array(x, MARGIN, fromLast)` — finds duplicated rows/columns in an array.
///
/// - `x`: array or matrix
/// - `MARGIN`: which margin to check (1=rows, 2=cols, etc.)
/// - `fromLast`: if TRUE, last occurrence is original
pub unsafe fn do_duplicated_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }

        // Parse MARGIN (default = 1, i.e. rows)
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

        let n = XLENGTH(x);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }

        // Get dimensions
        let dim = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );

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
        let dim_vals = INTEGER(dim);
        let nrows = *dim_vals as usize;
        let ncols = if dims_len >= 2 {
            (*dim_vals.add(1)) as usize
        } else {
            1
        };

        // For 2D arrays, support MARGIN=1 (rows) and MARGIN=2 (columns)
        if margin == 1 && dims_len == 2 {
            // Duplicate rows
            let total = nrows;
            let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = LOGICAL(result);

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
                        *dst.add(row) = TRUE;
                    } else {
                        encountered.insert(row_strings[row].clone());
                        *dst.add(row) = FALSE;
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
                        *dst.add(row) = TRUE;
                    } else {
                        seen.insert(key);
                        *dst.add(row) = FALSE;
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
            let dst = LOGICAL(result);

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
                        *dst.add(col) = TRUE;
                    } else {
                        encountered.insert(col_strings[col].clone());
                        *dst.add(col) = FALSE;
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
                        *dst.add(col) = TRUE;
                    } else {
                        seen.insert(key);
                        *dst.add(col) = FALSE;
                    }
                }
            }

            result
        } else {
            // Generic: flatten along margin — fallback to duplicated on flattened vector
            // For higher-dimensional arrays, treat as 1D
            let mut new_args = R_NilValue();
            new_args = Rf_cons(Rf_ScalarInteger(NA_INTEGER), new_args);
            new_args = Rf_cons(
                Rf_ScalarLogical(if from_last { TRUE } else { FALSE }),
                new_args,
            );
            new_args = Rf_cons(R_NilValue(), new_args);
            new_args = Rf_cons(x, new_args);
            do_duplicated(_call, _op, new_args, _rho)
        }
    }
}

// ---------------------------------------------------------------------------
// do_anyDuplicated.array — check for any duplicates in array along margin
// ---------------------------------------------------------------------------

/// R's `anyDuplicated.array(x, MARGIN, fromLast)` — returns index of first duplicate in array (0 if none).
pub unsafe fn do_anyDuplicated_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
        let dim = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );

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
        let dim_vals = INTEGER(dim);
        let nrows = *dim_vals as usize;
        let ncols = if dims_len >= 2 {
            (*dim_vals.add(1)) as usize
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
pub unsafe fn do_match(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = match_arg(args, 0, "x", R_NilValue());
        let table = match_arg(args, 1, "table", R_NilValue());
        let nomatch_arg = match_arg(args, 2, "nomatch", Rf_ScalarInteger(NA_INTEGER));
        let incomparables = match_arg(args, 3, "incomparables", R_NilValue());
        let nomatch = integer_scalar_or(nomatch_arg, NA_INTEGER);

        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);

        let common_type = match_common_type(x, table);
        let mut incomparable_set = BTreeSet::new();
        if !incomparables.is_null() && incomparables != R_NilValue() {
            for i in 0..XLENGTH(incomparables) {
                incomparable_set.insert(match_key(incomparables, i, common_type));
            }
        }

        let mut lookup: BTreeMap<MatchKey, c_int> = BTreeMap::new();
        if !table.is_null() && table != R_NilValue() {
            let tn = XLENGTH(table);
            for i in 0..tn {
                lookup
                    .entry(match_key(table, i, common_type))
                    .or_insert((i + 1) as c_int);
            }
        }
        for i in 0..n {
            let key = match_key(x, i, common_type);
            *dst.add(i as usize) = if incomparable_set.contains(&key) {
                nomatch
            } else {
                *lookup.get(&key).unwrap_or(&nomatch)
            };
        }
        result
    }
}

unsafe fn integer_scalar_or(arg: SEXP, default: c_int) -> c_int {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || arg == R_MissingArg() {
            return default;
        }
        match SEXPTYPE(TYPEOF(arg)) {
            SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => {
                if XLENGTH(arg) < 1 {
                    default
                } else {
                    INTEGER_ELT(arg, 0)
                }
            }
            SEXPTYPE::REALSXP => {
                if XLENGTH(arg) < 1 {
                    default
                } else {
                    let value = REAL_ELT(arg, 0);
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
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum MatchKey {
    Missing,
    String(String),
    Integer(c_int),
    Real(u64),
}

fn match_common_type(x: SEXP, table: SEXP) -> SEXPTYPE {
    unsafe {
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
        } else if xtype == SEXPTYPE::REALSXP || ttype == SEXPTYPE::REALSXP {
            SEXPTYPE::REALSXP
        } else {
            SEXPTYPE::INTSXP
        }
    }
}

unsafe fn match_key(x: SEXP, index: R_xlen_t, common_type: SEXPTYPE) -> MatchKey {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return MatchKey::Missing;
        }
        match common_type {
            SEXPTYPE::STRSXP => {
                if TYPEOF(x) == SEXPTYPE::STRSXP
                    && STRING_ELT(x, index) == crate::sexp::globals::R_NaString()
                {
                    MatchKey::Missing
                } else {
                    MatchKey::String(elt_to_string(x, index))
                }
            }
            SEXPTYPE::REALSXP => {
                let value = match TYPEOF(x) {
                    t if t == SEXPTYPE::REALSXP => REAL_ELT(x, index as c_int),
                    t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                        let value = INTEGER_ELT(x, index as c_int);
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
                    MatchKey::Real(value.to_bits())
                }
            }
            _ => {
                let value = match TYPEOF(x) {
                    t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                        INTEGER_ELT(x, index as c_int)
                    }
                    t if t == SEXPTYPE::REALSXP => {
                        let value = REAL_ELT(x, index as c_int);
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
}

unsafe fn match_arg(args: SEXP, position: usize, name: &str, default: SEXP) -> SEXP {
    unsafe {
        if let Some(value) = named_arg(args, name) {
            return value;
        }
        let mut positional = 0usize;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).is_none() {
                if positional == position {
                    let value = CAR(current);
                    return if value.is_null() || value == R_MissingArg() {
                        default
                    } else {
                        value
                    };
                }
                positional += 1;
            }
            current = CDR(current);
        }
        default
    }
}

// ---------------------------------------------------------------------------
// do_findInterval — find interval in sorted vector
// ---------------------------------------------------------------------------

/// R's `findInterval(x, vec)` — for each x, find j such that vec[j] <= x < vec[j+1].
pub unsafe fn do_findInterval(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        let vec = arg_by_name_or_position(args, &["vec"], 1);
        let rightmost_closed =
            logical_arg_by_name_or_position(args, "rightmost.closed", 2).unwrap_or(false);
        let all_inside = logical_arg_by_name_or_position(args, "all.inside", 3).unwrap_or(false);
        let left_open = logical_arg_by_name_or_position(args, "left.open", 4).unwrap_or(false);
        if x.is_null() || x == R_NilValue() || vec.is_null() || vec == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let n = XLENGTH(x);
        let vn = XLENGTH(vec);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);
        let mut vvals: Vec<f64> = Vec::with_capacity(vn as usize);
        for i in 0..vn {
            vvals.push(elt_real_safe(vec, i));
        }
        for i in 0..n {
            let xi = elt_real_safe(x, i);
            if xi.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || xi.is_nan() {
                *dst.add(i as usize) = NA_INTEGER;
                continue;
            }
            if vn == 0 {
                *dst.add(i as usize) = 0;
                continue;
            }
            if vn == 1 {
                let b = vvals[0];
                let interval = if all_inside {
                    if xi < b || (left_open && xi == b) {
                        1
                    } else {
                        0
                    }
                } else if rightmost_closed && xi == b {
                    if left_open { 1 } else { 0 }
                } else if left_open {
                    if xi > b { 1 } else { 0 }
                } else if xi >= b {
                    1
                } else {
                    0
                };
                *dst.add(i as usize) = interval;
                continue;
            }
            let mut lo = 0usize;
            let mut hi = vvals.len();
            while lo < hi {
                let mid = (lo + hi) / 2;
                let before_or_at = if left_open {
                    vvals[mid] < xi
                } else {
                    vvals[mid] <= xi
                };
                if before_or_at {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            let mut interval = lo as c_int;
            if rightmost_closed {
                if !left_open && xi == vvals[vvals.len() - 1] {
                    interval = (vn - 1) as c_int;
                } else if left_open && xi == vvals[0] {
                    interval = 1;
                }
            }
            if all_inside && vn > 1 {
                interval = interval.clamp(1, (vn - 1) as c_int);
            }
            *dst.add(i as usize) = interval;
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_cut — cut numeric vector into intervals
// ---------------------------------------------------------------------------

/// R's `cut(x, breaks)` — cut a numeric vector into interval factor codes.
pub unsafe fn do_cut(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        let breaks_arg = arg_by_name_or_position(args, &["breaks"], 1);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let n = XLENGTH(x);
        let mut break_pts: Vec<f64> = Vec::new();
        if !breaks_arg.is_null() && breaks_arg != R_NilValue() {
            let bt = TYPEOF(breaks_arg);
            if bt == SEXPTYPE::INTSXP || bt == SEXPTYPE::REALSXP {
                let bn = XLENGTH(breaks_arg);
                if bn == 1 {
                    let nbins = elt_real_safe(breaks_arg, 0) as i64;
                    if nbins < 1 {
                        return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
                    }
                    let mut lo = f64::INFINITY;
                    let mut hi = f64::NEG_INFINITY;
                    for i in 0..n {
                        let v = elt_real_safe(x, i);
                        if v.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN && !v.is_nan() {
                            if v < lo {
                                lo = v;
                            }
                            if v > hi {
                                hi = v;
                            }
                        }
                    }
                    if lo == f64::INFINITY {
                        lo = 0.0;
                        hi = 1.0;
                    }
                    let step = (hi - lo) / nbins as f64;
                    for i in 0..=nbins {
                        break_pts.push(lo + i as f64 * step);
                    }
                    if let Some(last) = break_pts.last_mut() {
                        *last += step * 0.001;
                    }
                } else {
                    for i in 0..bn {
                        break_pts.push(elt_real_safe(breaks_arg, i));
                    }
                }
            }
        }
        if break_pts.len() < 2 {
            break_pts = vec![0.0, 1.0];
        }
        let right = logical_arg_by_name_or_position(args, "right", 3).unwrap_or(true);
        let include_lowest =
            logical_arg_by_name_or_position(args, "include.lowest", 4).unwrap_or(false);
        let labels_arg = arg_by_name_or_position(args, &["labels"], 2);
        let labels_false = if labels_arg.is_null() || labels_arg == R_NilValue() {
            false
        } else if TYPEOF(labels_arg) == SEXPTYPE::LGLSXP && XLENGTH(labels_arg) > 0 {
            *LOGICAL(labels_arg) == FALSE
        } else {
            false
        };
        let levels = if labels_arg.is_null()
            || labels_arg == R_NilValue()
            || (TYPEOF(labels_arg) == SEXPTYPE::LGLSXP && XLENGTH(labels_arg) > 0)
        {
            cut_interval_labels(&break_pts, right, include_lowest)
        } else {
            (0..XLENGTH(labels_arg))
                .map(|i| elt_to_string(labels_arg, i))
                .collect::<Vec<_>>()
        };

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let v = elt_real_safe(x, i);
            *INTEGER(result).add(i as usize) =
                cut_interval_code(v, &break_pts, right, include_lowest);
        }
        if !labels_false {
            set_factor_attrs(result, &levels);
        }
        result
    }
}

fn cut_interval_code(value: f64, breaks: &[f64], right: bool, include_lowest: bool) -> c_int {
    if value.to_bits() == R_NA_BIT_PATTERN || value.is_nan() {
        return NA_INTEGER;
    }
    for interval in 0..breaks.len().saturating_sub(1) {
        let lower = breaks[interval];
        let upper = breaks[interval + 1];
        let contains = if right {
            let lower_ok = value > lower || (include_lowest && interval == 0 && value == lower);
            lower_ok && value <= upper
        } else {
            let upper_ok =
                value < upper || (include_lowest && interval + 2 == breaks.len() && value == upper);
            value >= lower && upper_ok
        };
        if contains {
            return interval as c_int + 1;
        }
    }
    NA_INTEGER
}

fn cut_interval_labels(breaks: &[f64], right: bool, include_lowest: bool) -> Vec<String> {
    let mut labels = Vec::with_capacity(breaks.len().saturating_sub(1));
    for interval in 0..breaks.len().saturating_sub(1) {
        let left_bracket = if right && include_lowest && interval == 0 {
            "["
        } else if right {
            "("
        } else {
            "["
        };
        let right_bracket = if !right && include_lowest && interval + 2 == breaks.len() {
            "]"
        } else if right {
            "]"
        } else {
            ")"
        };
        labels.push(format!(
            "{}{},{}{}",
            left_bracket,
            format_cut_number(breaks[interval]),
            format_cut_number(breaks[interval + 1]),
            right_bracket
        ));
    }
    labels
}

fn format_cut_number(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    }
}

// ---------------------------------------------------------------------------
// Set operations: unique, sort, order, rev, match, %in%, setequal, union, intersect, setdiff
// ---------------------------------------------------------------------------

/// R's `unique(x)` — return unique atomic elements in R's retained-index order.
pub unsafe fn do_unique(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        let sexptype = SEXPTYPE(t);
        if t != SEXPTYPE::LGLSXP
            && t != SEXPTYPE::INTSXP
            && t != SEXPTYPE::REALSXP
            && t != SEXPTYPE::STRSXP
        {
            return x;
        }
        let n = XLENGTH(x);
        let from_last = logical_arg_by_name_or_position(args, "fromLast", 2).unwrap_or(false);
        let incomparables = arg_by_name_or_position(args, &["incomparables"], 1);
        let incomparable_keys = atomic_incomparable_keys(incomparables, sexptype);

        let mut unique_indices: Vec<R_xlen_t> = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        if from_last {
            let mut keep = vec![false; n as usize];
            for i in (0..n).rev() {
                let key = atomic_unique_key(x, i, sexptype);
                if incomparable_keys.contains(&key) || seen.insert(key) {
                    keep[i as usize] = true;
                }
            }
            for i in 0..n {
                if keep[i as usize] {
                    unique_indices.push(i);
                }
            }
        } else {
            for i in 0..n {
                let key = atomic_unique_key(x, i, sexptype);
                if incomparable_keys.contains(&key) || seen.insert(key) {
                    unique_indices.push(i);
                }
            }
        }

        let result = Rf_allocVector3(t, unique_indices.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (new_i, &old_i) in unique_indices.iter().enumerate() {
            match t {
                tt if tt == SEXPTYPE::REALSXP => {
                    *REAL(result).add(new_i) = *REAL(x).add(old_i as usize);
                }
                tt if tt == SEXPTYPE::STRSXP => {
                    SET_STRING_ELT(result, new_i as R_xlen_t, STRING_ELT(x, old_i));
                }
                _ => {
                    *INTEGER(result).add(new_i) = *INTEGER(x).add(old_i as usize);
                }
            }
        }
        result
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
enum AtomicUniqueKey {
    Integer(c_int),
    Real(u64),
    String(String),
}

fn atomic_incomparable_keys(
    incomparables: SEXP,
    target_type: SEXPTYPE,
) -> std::collections::BTreeSet<AtomicUniqueKey> {
    unsafe {
        let mut keys = std::collections::BTreeSet::new();
        if incomparables.is_null() || incomparables == R_NilValue() {
            return keys;
        }
        let n = XLENGTH(incomparables);
        for i in 0..n {
            keys.insert(atomic_unique_key(incomparables, i, target_type));
        }
        keys
    }
}

fn atomic_unique_key(x: SEXP, index: R_xlen_t, target_type: SEXPTYPE) -> AtomicUniqueKey {
    unsafe {
        match target_type {
            t if t == SEXPTYPE::STRSXP => {
                if TYPEOF(x) == SEXPTYPE::STRSXP {
                    let value = STRING_ELT(x, index);
                    if charsxp_is_na(value) {
                        AtomicUniqueKey::String("<NA>".to_string())
                    } else {
                        AtomicUniqueKey::String(elt_to_string(x, index))
                    }
                } else if atomic_value_is_missing(x, index) {
                    AtomicUniqueKey::String("<NA>".to_string())
                } else {
                    AtomicUniqueKey::String(elt_to_string(x, index))
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                let value = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(index as usize)
                } else {
                    let raw = *INTEGER(x).add(index as usize);
                    if raw == NA_INTEGER {
                        NA_REAL
                    } else {
                        raw as f64
                    }
                };
                if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    AtomicUniqueKey::Real(crate::sexp::ffi::R_NA_BIT_PATTERN)
                } else if value.is_nan() {
                    AtomicUniqueKey::Real(f64::NAN.to_bits())
                } else {
                    AtomicUniqueKey::Real(value.to_bits())
                }
            }
            _ => {
                let value = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    let value = *REAL(x).add(index as usize);
                    if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        NA_INTEGER
                    } else {
                        value as c_int
                    }
                } else {
                    *INTEGER(x).add(index as usize)
                };
                AtomicUniqueKey::Integer(value)
            }
        }
    }
}

pub(crate) fn atomic_value_is_missing(x: SEXP, index: R_xlen_t) -> bool {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP => charsxp_is_na(STRING_ELT(x, index)),
            t if t == SEXPTYPE::REALSXP => {
                let value = *REAL(x).add(index as usize);
                value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || value.is_nan()
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                *INTEGER(x).add(index as usize) == NA_INTEGER
            }
            _ => false,
        }
    }
}

fn copy_atomic_element(
    dst: SEXP,
    dst_index: R_xlen_t,
    src: SEXP,
    src_index: R_xlen_t,
    target_type: SEXPTYPE,
) {
    unsafe {
        match target_type {
            t if t == SEXPTYPE::STRSXP => {
                if TYPEOF(src) == SEXPTYPE::STRSXP {
                    SET_STRING_ELT(dst, dst_index, STRING_ELT(src, src_index));
                } else {
                    let text = elt_to_string(src, src_index);
                    let cstr = CString::new(text).unwrap_or_default();
                    SET_STRING_ELT(
                        dst,
                        dst_index,
                        crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()),
                    );
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                let value = if TYPEOF(src) == SEXPTYPE::REALSXP {
                    *REAL(src).add(src_index as usize)
                } else {
                    let raw = *INTEGER(src).add(src_index as usize);
                    if raw == NA_INTEGER {
                        NA_REAL
                    } else {
                        raw as f64
                    }
                };
                *REAL(dst).add(dst_index as usize) = value;
            }
            _ => {
                let value = if TYPEOF(src) == SEXPTYPE::REALSXP {
                    let value = *REAL(src).add(src_index as usize);
                    if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        NA_INTEGER
                    } else {
                        value as c_int
                    }
                } else {
                    *INTEGER(src).add(src_index as usize)
                };
                *INTEGER(dst).add(dst_index as usize) = value;
            }
        }
    }
}

/// R's `sort(x, decreasing, na.last)` — sort an atomic vector.
pub unsafe fn do_sort(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let decreasing = sort_logical_arg(args, &["decreasing"], 1).unwrap_or(false);
        let na_placement = sort_na_placement(args);

        let t = TYPEOF(x);
        let n = XLENGTH(x);
        if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let mut vals: Vec<i32> = Vec::with_capacity(n as usize);
            let mut na_count = 0usize;
            for i in 0..n {
                let value = *INTEGER(x).add(i as usize);
                if value == NA_INTEGER {
                    na_count += 1;
                } else {
                    vals.push(value);
                }
            }
            if decreasing {
                vals.sort_by(|a, b| b.cmp(a));
            } else {
                vals.sort_unstable();
            }
            let output_len = sorted_len(vals.len(), na_count, na_placement);
            let result = Rf_allocVector3(t, output_len as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = INTEGER(result);
            let mut out = 0usize;
            if na_placement == SortNaPlacement::First {
                for _ in 0..na_count {
                    *dst.add(out) = NA_INTEGER;
                    out += 1;
                }
            }
            for value in vals {
                *dst.add(out) = value;
                out += 1;
            }
            if na_placement == SortNaPlacement::Last {
                for _ in 0..na_count {
                    *dst.add(out) = NA_INTEGER;
                    out += 1;
                }
            }
            result
        } else if t == SEXPTYPE::REALSXP {
            let mut vals: Vec<f64> = Vec::with_capacity(n as usize);
            let mut na_count = 0usize;
            for i in 0..n {
                let value = *REAL(x).add(i as usize);
                if ISNAN(value) {
                    na_count += 1;
                } else {
                    vals.push(value);
                }
            }
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            if decreasing {
                vals.reverse();
            }
            let output_len = sorted_len(vals.len(), na_count, na_placement);
            let result = Rf_allocVector3(t, output_len as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = REAL(result);
            let mut out = 0usize;
            if na_placement == SortNaPlacement::First {
                for _ in 0..na_count {
                    *dst.add(out) = NA_REAL;
                    out += 1;
                }
            }
            for value in vals {
                *dst.add(out) = value;
                out += 1;
            }
            if na_placement == SortNaPlacement::Last {
                for _ in 0..na_count {
                    *dst.add(out) = NA_REAL;
                    out += 1;
                }
            }
            result
        } else if t == SEXPTYPE::STRSXP {
            let mut vals: Vec<SEXP> = Vec::with_capacity(n as usize);
            let mut na_count = 0usize;
            for i in 0..n {
                let value = STRING_ELT(x, i);
                if charsxp_is_na(value) {
                    na_count += 1;
                } else {
                    vals.push(value);
                }
            }
            vals.sort_by(|a, b| compare_charsxp_for_sort(*a, *b));
            if decreasing {
                vals.reverse();
            }
            let output_len = sorted_len(vals.len(), na_count, na_placement);
            let result = Rf_allocVector3(t, output_len as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let mut out = 0usize;
            if na_placement == SortNaPlacement::First {
                for _ in 0..na_count {
                    SET_STRING_ELT(result, out as R_xlen_t, crate::sexp::globals::R_NaString());
                    out += 1;
                }
            }
            for value in vals {
                SET_STRING_ELT(result, out as R_xlen_t, value);
                out += 1;
            }
            if na_placement == SortNaPlacement::Last {
                for _ in 0..na_count {
                    SET_STRING_ELT(result, out as R_xlen_t, crate::sexp::globals::R_NaString());
                    out += 1;
                }
            }
            result
        } else {
            let result = Rf_allocVector3(t, n);
            if result.is_null() {
                return R_NilValue();
            }
            result
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum SortNaPlacement {
    Remove,
    Last,
    First,
}

fn sorted_len(value_count: usize, na_count: usize, na_placement: SortNaPlacement) -> usize {
    value_count
        + match na_placement {
            SortNaPlacement::Remove => 0,
            SortNaPlacement::Last | SortNaPlacement::First => na_count,
        }
}

fn sort_na_placement(args: SEXP) -> SortNaPlacement {
    match sort_logical_arg(args, &["na.last"], 2) {
        Some(true) => SortNaPlacement::Last,
        Some(false) => SortNaPlacement::First,
        None => SortNaPlacement::Remove,
    }
}

pub(crate) fn sort_logical_arg(args: SEXP, names: &[&str], position: usize) -> Option<bool> {
    unsafe {
        let arg = arg_by_name_or_position(args, names, position);
        if arg.is_null() || arg == R_NilValue() || XLENGTH(arg) == 0 {
            return None;
        }
        let raw = if TYPEOF(arg) == SEXPTYPE::LGLSXP || TYPEOF(arg) == SEXPTYPE::INTSXP {
            *INTEGER(arg)
        } else if TYPEOF(arg) == SEXPTYPE::REALSXP {
            let value = *REAL(arg);
            if ISNAN(value) {
                NA_LOGICAL
            } else {
                value as c_int
            }
        } else {
            return None;
        };
        (raw != NA_LOGICAL).then_some(raw != 0)
    }
}

pub(crate) fn charsxp_is_na(value: SEXP) -> bool {
    unsafe { value.is_null() || value == crate::sexp::globals::R_NaString() }
}

fn compare_charsxp_for_sort(a: SEXP, b: SEXP) -> std::cmp::Ordering {
    unsafe {
        let a_is_na = charsxp_is_na(a);
        let b_is_na = charsxp_is_na(b);
        match (a_is_na, b_is_na) {
            (true, true) => return std::cmp::Ordering::Equal,
            (true, false) => return std::cmp::Ordering::Greater,
            (false, true) => return std::cmp::Ordering::Less,
            (false, false) => {}
        }
        let a_ptr = CHAR(a);
        let b_ptr = CHAR(b);
        let a_text = if a_ptr.is_null() {
            ""
        } else {
            std::ffi::CStr::from_ptr(a_ptr).to_str().unwrap_or("")
        };
        let b_text = if b_ptr.is_null() {
            ""
        } else {
            std::ffi::CStr::from_ptr(b_ptr).to_str().unwrap_or("")
        };
        a_text.cmp(b_text)
    }
}

/// R's `rev(x)` — reverse a vector.
pub unsafe fn do_rev(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(t, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            let src = (n - 1 - i) as usize;
            let dst = i as usize;
            if t == SEXPTYPE::REALSXP {
                *REAL(result).add(dst) = *REAL(x).add(src);
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                *INTEGER(result).add(dst) = *INTEGER(x).add(src);
            } else if t == SEXPTYPE::STRSXP {
                SET_STRING_ELT(result, i, STRING_ELT(x, src as R_xlen_t));
            } else if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP {
                SET_VECTOR_ELT(result, i, VECTOR_ELT(x, src as R_xlen_t));
            } else if t == SEXPTYPE::RAWSXP {
                *RAW(result).add(dst) = *RAW(x).add(src);
            }
        }
        reverse_names_attribute(x, result, n);
        result
    }
}

unsafe fn reverse_names_attribute(x: SEXP, result: SEXP, len: R_xlen_t) {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return;
        }
        let reversed = Rf_allocVector3(SEXPTYPE::STRSXP, len);
        if reversed.is_null() {
            return;
        }
        let _reversed_guard = protect(reversed);
        for i in 0..len {
            let src = len - 1 - i;
            SET_STRING_ELT(reversed, i, STRING_ELT(names, src));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            reversed,
        );
    }
}

unsafe fn logical_arg_value(x: SEXP, index: R_xlen_t) -> Option<c_int> {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP.as_c_int() || t == SEXPTYPE::INTSXP.as_c_int() => {
                Some(integer_or_logical_elt(x, index as c_int))
            }
            t if t == SEXPTYPE::REALSXP.as_c_int() => {
                let value = *REAL(x).add(index as usize);
                if value.is_nan() {
                    Some(NA_INTEGER)
                } else {
                    Some((value != 0.0) as c_int)
                }
            }
            _ => None,
        }
    }
}

unsafe fn logical_na_rm_from_args(args: SEXP) -> bool {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some("na.rm") {
                let value = CAR(current);
                if !value.is_null() && value != R_NilValue() && XLENGTH(value) > 0 {
                    return logical_arg_value(value, 0) == Some(TRUE);
                }
            }
            current = CDR(current);
        }
        false
    }
}

/// R's `any(...)` — TRUE if any element is TRUE.
pub unsafe fn do_any(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let na_rm = logical_na_rm_from_args(args);
        let mut has_na = false;
        let mut current = args;

        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some("na.rm") {
                current = CDR(current);
                continue;
            }

            let x = CAR(current);
            if !x.is_null() && x != R_NilValue() {
                let n = XLENGTH(x);
                for i in 0..n {
                    match logical_arg_value(x, i) {
                        Some(TRUE) => return Rf_ScalarLogical(TRUE),
                        Some(NA_INTEGER) if !na_rm => has_na = true,
                        Some(_) | None => {}
                    }
                }
            }
            current = CDR(current);
        }

        Rf_ScalarLogical(if has_na { NA_INTEGER } else { FALSE })
    }
}

/// R's `all(...)` — TRUE if all elements are TRUE.
pub unsafe fn do_all(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let na_rm = logical_na_rm_from_args(args);
        let mut has_na = false;
        let mut current = args;

        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some("na.rm") {
                current = CDR(current);
                continue;
            }

            let x = CAR(current);
            if !x.is_null() && x != R_NilValue() {
                let n = XLENGTH(x);
                for i in 0..n {
                    match logical_arg_value(x, i) {
                        Some(FALSE) => return Rf_ScalarLogical(FALSE),
                        Some(NA_INTEGER) if !na_rm => has_na = true,
                        Some(_) | None => {}
                    }
                }
            }
            current = CDR(current);
        }

        Rf_ScalarLogical(if has_na { NA_INTEGER } else { TRUE })
    }
}

/// R's `cumsum(x)` — cumulative sum.
pub unsafe fn do_cumsum(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
        let _result_guard = protect(result);

        if result_type == SEXPTYPE::INTSXP {
            let dst = INTEGER(result);
            let mut sum = 0_i64;
            let mut poisoned = false;
            let mut warned = false;
            for i in 0..n {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER {
                    poisoned = true;
                }
                if poisoned {
                    *dst.add(i as usize) = NA_INTEGER;
                } else {
                    sum += v as i64;
                    if sum > i32::MAX as i64 || sum < i32::MIN as i64 {
                        poisoned = true;
                        *dst.add(i as usize) = NA_INTEGER;
                        if !warned {
                            warned = true;
                            let msg = CString::new(
                                "integer overflow in 'cumsum'; use 'cumsum(as.numeric(.))'",
                            )
                            .unwrap_or_default();
                            crate::mainutils::errors::Rf_warning(msg.as_ptr());
                        }
                    } else {
                        *dst.add(i as usize) = sum as c_int;
                    }
                }
            }
            return result;
        }

        let dst = REAL(result);
        let mut sum = 0.0f64;
        let mut poisoned = false;
        for i in 0..n {
            let v = elt_real_safe(x, i);
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                poisoned = true;
            }
            if poisoned {
                *dst.add(i as usize) = NA_REAL;
            } else {
                sum += v;
                *dst.add(i as usize) = sum;
            }
        }
        result
    }
}

/// R's `cumprod(x)` — cumulative product.
pub unsafe fn do_cumprod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        let mut prod = 1.0f64;
        let mut poisoned = false;
        for i in 0..n {
            let v = elt_real_safe(x, i);
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                poisoned = true;
            }
            if poisoned {
                *dst.add(i as usize) = NA_REAL;
            } else {
                prod *= v;
                *dst.add(i as usize) = prod;
            }
        }
        result
    }
}

/// R's `cumvar(x)` — cumulative sample variance by Youngs-Cramer algorithm.
pub unsafe fn do_cumvar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(x) == SEXPTYPE::CPLXSXP {
            base_error("'cumvar' not defined for complex numbers");
        }

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        if n == 0 {
            return result;
        }

        *dst = NA_REAL;
        let mut var = 0.0f64;
        let mut sum = elt_real_safe(x, 0);
        for i in 1..n {
            let value = elt_real_safe(x, i);
            sum += value;
            let count = (i + 1) as f64;
            let numerator = count * value - sum;
            var += numerator.powi(2) / (i as f64 * count);
            *dst.add(i as usize) = var / i as f64;
        }
        result
    }
}

/// R's `diff(x, lag)` — lagged differences.
pub unsafe fn do_diff(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let lag_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let lag = if lag_arg.is_null() || lag_arg == R_NilValue() {
            1
        } else {
            real_or_default(lag_arg, 1.0) as usize
        };
        let n = XLENGTH(x);
        if n <= lag as R_xlen_t {
            let empty_type = if TYPEOF(x) == SEXPTYPE::INTSXP || TYPEOF(x) == SEXPTYPE::LGLSXP {
                SEXPTYPE::INTSXP
            } else {
                SEXPTYPE::REALSXP
            };
            return Rf_allocVector3(empty_type, 0);
        }
        let result_len = n - lag as R_xlen_t;
        let result_type = if TYPEOF(x) == SEXPTYPE::INTSXP || TYPEOF(x) == SEXPTYPE::LGLSXP {
            SEXPTYPE::INTSXP
        } else {
            SEXPTYPE::REALSXP
        };
        let result = Rf_allocVector3(result_type, result_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        if result_type == SEXPTYPE::INTSXP {
            let dst = INTEGER(result);
            let mut warned = false;
            for i in 0..result_len {
                let a = *INTEGER(x).add(i as usize);
                let b = *INTEGER(x).add((i + lag as R_xlen_t) as usize);
                *dst.add(i as usize) = if a == NA_INTEGER || b == NA_INTEGER {
                    NA_INTEGER
                } else {
                    let diff = b as i64 - a as i64;
                    if diff > i32::MAX as i64 || diff < i32::MIN as i64 {
                        if !warned {
                            warned = true;
                            let msg = CString::new("NAs produced by integer overflow")
                                .unwrap_or_default();
                            crate::mainutils::errors::Rf_warning(msg.as_ptr());
                        }
                        NA_INTEGER
                    } else {
                        diff as c_int
                    }
                };
            }
            return result;
        }

        let dst = REAL(result);
        for i in 0..result_len {
            let a = elt_real_safe(x, i);
            let b = elt_real_safe(x, i + lag as R_xlen_t);
            *dst.add(i as usize) = b - a;
        }
        result
    }
}
