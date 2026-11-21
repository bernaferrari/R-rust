use super::*;
use crate::sexp::object::SexpMut;

pub(crate) fn match_arg(args: SEXP, position: usize, name: &str, default: SEXP) -> SEXP {
    {
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
pub fn do_findInterval(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
    let owner = active_owner();
    let mut dst = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let mut vvals: Vec<f64> = Vec::with_capacity(vn as usize);
    for i in 0..vn {
        vvals.push(elt_real_safe(vec, i));
    }
    for i in 0..n {
        let xi = elt_real_safe(x, i);
        if xi.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || xi.is_nan() {
            dst.try_set_integer_elt(i, NA_INTEGER)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            continue;
        }
        if vn == 0 {
            dst.try_set_integer_elt(i, 0)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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
            dst.try_set_integer_elt(i, interval)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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
        dst.try_set_integer_elt(i, interval)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    result
}

// ---------------------------------------------------------------------------
// do_cut — cut numeric vector into intervals
// ---------------------------------------------------------------------------

/// R's `cut(x, breaks)` — cut a numeric vector into interval factor codes.
pub fn do_cut(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        let x = arg_by_name_or_position(args, &["x"], 0);
        let breaks_arg = arg_by_name_or_position(args, &["breaks"], 1);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        if sexp_has_class(x, "Date")
            || sexp_has_class(x, "POSIXct")
            || sexp_has_class(x, "POSIXt")
            || sexp_has_class(x, "POSIXlt")
        {
            if sexp_has_class(x, "Date") {
                return do_cut_Date(_call, _op, args, _rho);
            }
            return do_cut_POSIXt(_call, _op, args, _rho);
        }
        if sexp_has_class(x, "dendrogram") {
            let sym = install_str("cut.dendrogram");
            let fun = crate::eval::eval::Rf_eval(sym, crate::sexp::globals::R_BaseEnv());
            if !fun.is_null() && fun != R_NilValue() && TYPEOF(fun) == SEXPTYPE::CLOSXP {
                return crate::eval::closure::applyClosure(_call, fun, args, _rho, _rho, 0);
            }
        }

        let n = XLENGTH(x);
        let mut break_pts: Vec<f64> = Vec::new();
        if !breaks_arg.is_null() && breaks_arg != R_NilValue() {
            let bt = TYPEOF(breaks_arg);
            if bt == SEXPTYPE::INTSXP || bt == SEXPTYPE::REALSXP {
                let bn = XLENGTH(breaks_arg);
                if bn == 1 {
                    let nbins = elt_real_safe(breaks_arg, 0) as i64;
                    if nbins < 2 {
                        base_error("invalid number of intervals".to_string());
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
                    let mut dx = hi - lo;
                    if dx == 0.0 {
                        dx = if lo != 0.0 { lo.abs() } else { 1.0 };
                        let start = lo - dx / 1000.0;
                        let end = hi + dx / 1000.0;
                        let step = (end - start) / nbins as f64;
                        for i in 0..=nbins {
                            break_pts.push(start + i as f64 * step);
                        }
                    } else {
                        // seq from the range, then pull the outer breaks
                        // past the data so the minimum and maximum are inside.
                        let step = dx / nbins as f64;
                        for i in 0..=nbins {
                            break_pts.push(lo + i as f64 * step);
                        }
                        break_pts[0] = lo - dx / 1000.0;
                        let last = break_pts.len() - 1;
                        break_pts[last] = hi + dx / 1000.0;
                    }
                } else {
                    for i in 0..bn {
                        break_pts.push(elt_real_safe(breaks_arg, i));
                    }
                }
            } else {
                base_error("invalid 'breaks' argument".to_string());
            }
        }
        let breaks_missing =
            breaks_arg.is_null() || breaks_arg == crate::sexp::globals::R_MissingArg();
        if break_pts.len() < 2 {
            if breaks_arg == R_NilValue() || !breaks_missing {
                base_error("invalid 'breaks' argument".to_string());
            } else {
                break_pts = vec![0.0, 1.0];
            }
        }
        let right = logical_arg_by_name_or_position(args, "right", 3).unwrap_or(true);
        let include_lowest =
            logical_arg_by_name_or_position(args, "include.lowest", 4).unwrap_or(false);
        let labels_arg = arg_by_name_or_position(args, &["labels"], 2);
        let labels_false = if labels_arg.is_null() || labels_arg == R_NilValue() {
            false
        } else if TYPEOF(labels_arg) == SEXPTYPE::LGLSXP && XLENGTH(labels_arg) > 0 {
            let owner = active_owner();
            owner_sexp(owner, labels_arg)
                .try_logical_elt(0)
                .unwrap_or(TRUE)
                == FALSE
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
        let owner = active_owner();
        let mut result_mut = owner
            .sexp(result)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        for i in 0..n {
            let v = elt_real_safe(x, i);
            let code = cut_interval_code(v, &break_pts, right, include_lowest);
            result_mut
                .try_set_integer_elt(i, code)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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
    // GNU base/cut.R: labels are paste0(bracket, ch.br[-nb], ",", ch.br[-1L],
    // bracket) where ch.br <- formatC(0+breaks, digits = dig, width = 1L).
    // formatC replaces non-finite values via as.character() and then
    // right-justifies that subset to its common width (format.default), so
    // e.g. c(-Inf, 0, Inf) formats to "-Inf", "0", " Inf".
    let n_breaks = breaks.len();
    let ch_br: Vec<String> = {
        let raw: Vec<String> = breaks
            .iter()
            .map(|&v| {
                if v.is_nan() {
                    "NA".to_string()
                } else if v.is_infinite() {
                    if v > 0.0 { "Inf" } else { "-Inf" }.to_string()
                } else {
                    format_cut_number(v)
                }
            })
            .collect();
        let special_width = raw
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                let v = breaks[*i];
                v.is_nan() || v.is_infinite()
            })
            .map(|(_, s)| s.chars().count())
            .max()
            .unwrap_or(0);
        raw.into_iter()
            .enumerate()
            .map(|(i, s)| {
                let v = breaks[i];
                if v.is_nan() || v.is_infinite() {
                    let len = s.chars().count();
                    if len < special_width {
                        let mut padded = " ".repeat(special_width - len);
                        padded.push_str(&s);
                        padded
                    } else {
                        s
                    }
                } else {
                    s
                }
            })
            .collect()
    };
    let mut labels = Vec::with_capacity(n_breaks.saturating_sub(1));
    for interval in 0..n_breaks.saturating_sub(1) {
        let left_bracket = if right && include_lowest && interval == 0 {
            "["
        } else if right {
            "("
        } else {
            "["
        };
        let right_bracket = if !right && include_lowest && interval + 2 == n_breaks {
            "]"
        } else if right {
            "]"
        } else {
            ")"
        };
        labels.push(format!(
            "{}{},{}{}",
            left_bracket,
            ch_br[interval],
            ch_br[interval + 1],
            right_bracket
        ));
    }
    labels
}

fn format_cut_number(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "Inf" } else { "-Inf" }.to_string()
    } else if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    }
}

// ---------------------------------------------------------------------------
// Set operations: unique, sort, order, rev, match, %in%, setequal, union, intersect, setdiff
// ---------------------------------------------------------------------------

fn unique_matrix_rows(x: SEXP, sexptype: SEXPTYPE) -> Option<SEXP> {
    let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() || TYPEOF(dim) != SEXPTYPE::INTSXP || XLENGTH(dim) != 2
    {
        return None;
    }
    let owner = active_owner();
    let dim_owned = owner_sexp(owner, dim);
    let nrow = dim_owned
        .try_integer_elt(0)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        as R_xlen_t;
    let ncol = dim_owned
        .try_integer_elt(1)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        as R_xlen_t;
    if nrow <= 0 || ncol <= 0 {
        return None;
    }
    let mut seen = BTreeSet::new();
    let mut keep: Vec<R_xlen_t> = Vec::new();
    for i in 0..nrow {
        let mut row = Vec::with_capacity(ncol as usize);
        for j in 0..ncol {
            let idx = i + j * nrow;
            row.push(atomic_unique_key(x, idx, sexptype));
        }
        if seen.insert(row) {
            keep.push(i);
        }
    }
    let nkeep = keep.len() as R_xlen_t;
    let result = Rf_allocVector3(TYPEOF(x), nkeep * ncol);
    let _r = protect(result);
    for (new_i, &old_i) in keep.iter().enumerate() {
        for j in 0..ncol {
            let src = old_i + j * nrow;
            let dst = new_i as R_xlen_t + j * nkeep;
            copy_atomic_element(result, dst, x, src, sexptype);
        }
    }
    let out_dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    let _d = protect(out_dim);
    if let Ok(mut out_dim_mut) = owner.sexp(out_dim).and_then(SexpMut::try_from_checked) {
        let _ = out_dim_mut.try_set_integer_elt(0, nkeep as c_int);
        let _ = out_dim_mut.try_set_integer_elt(1, ncol as c_int);
    }
    crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_DimSymbol(), out_dim);
    Some(result)
}

/// R's `unique(x)` — return unique atomic elements in R's retained-index order.
pub fn do_unique(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = arg_by_name_or_position(args, &["x"], 0);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(x);
    // GNU unique.c: VECSXP/EXPRSXP keep x[!duplicated(x)].
    if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP {
        let dup = do_duplicated(_call, _op, args, _rho);
        let _d = protect(dup);
        let n = XLENGTH(x);
        let mut keep: Vec<R_xlen_t> = Vec::new();
        if TYPEOF(dup) == SEXPTYPE::LGLSXP {
            let owner = active_owner();
            let dup_owned = owner_sexp(owner, dup);
            for i in 0..n {
                let v = dup_owned
                    .try_logical_elt(i)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                if v == FALSE {
                    keep.push(i);
                }
            }
        } else {
            for i in 0..n {
                keep.push(i);
            }
        }
        let result = Rf_allocVector3(t, keep.len() as R_xlen_t);
        let _r = protect(result);
        let owner = active_owner();
        let x_owned = owner_sexp(owner, x);
        let mut result_mut = owner
            .sexp(result)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        for (j, &i) in keep.iter().enumerate() {
            let elt = x_owned
                .try_vector_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            result_mut
                .try_set_vector_elt(j as R_xlen_t, elt)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        return result;
    }
    let sexptype = SEXPTYPE(t);
    if t != SEXPTYPE::LGLSXP
        && t != SEXPTYPE::INTSXP
        && t != SEXPTYPE::REALSXP
        && t != SEXPTYPE::STRSXP
    {
        return x;
    }
    if let Some(result) = unique_matrix_rows(x, sexptype) {
        return result;
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
        copy_atomic_element(result, new_i as R_xlen_t, x, old_i, sexptype);
    }
    restore_datetime_or_difftime_class(x, result);
    result
}

pub(crate) fn restore_datetime_or_difftime_class(source: SEXP, result: SEXP) {
    if result.is_null() || result == R_NilValue() {
        return;
    }
    if sexp_has_class(source, "Date") {
        set_datetime_class_from(result, source, DatetimeVectorClass::Date);
    } else if sexp_has_class(source, "POSIXct") || sexp_has_class(source, "POSIXt") {
        set_datetime_class_from(result, source, DatetimeVectorClass::Posixct);
    } else if sexp_has_class(source, "difftime") {
        let units = crate::sexp::attrib_core::getAttrib(source, install_str("units"));
        set_single_class(result, "difftime");
        if !units.is_null() && units != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(result, install_str("units"), units);
        }
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum AtomicUniqueKey {
    Integer(c_int),
    Real(u64),
    String(String),
}

fn atomic_incomparable_keys(
    incomparables: SEXP,
    target_type: SEXPTYPE,
) -> std::collections::BTreeSet<AtomicUniqueKey> {
    let mut keys = std::collections::BTreeSet::new();
    if incomparables.is_null() || incomparables == R_NilValue() {
        return keys;
    }
    // GNU: `incomparables = FALSE` means every value can be compared.
    if TYPEOF(incomparables) == SEXPTYPE::LGLSXP && XLENGTH(incomparables) == 1 && {
        let owner = active_owner();
        owner_sexp(owner, incomparables)
            .try_logical_elt(0)
            .unwrap_or(TRUE)
            == FALSE
    } {
        return keys;
    }
    let n = XLENGTH(incomparables);
    for i in 0..n {
        keys.insert(atomic_unique_key(incomparables, i, target_type));
    }
    keys
}

pub(crate) fn atomic_unique_key(
    x: SEXP,
    index: R_xlen_t,
    target_type: SEXPTYPE,
) -> AtomicUniqueKey {
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    match target_type {
        t if t == SEXPTYPE::STRSXP => {
            if TYPEOF(x) == SEXPTYPE::STRSXP {
                let value = x_owned
                    .try_string_elt(index)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    .as_raw();
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
                x_owned
                    .try_real_elt(index)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            } else {
                let raw = integer_like_elt(&x_owned, index);
                if raw == NA_INTEGER {
                    NA_REAL
                } else {
                    raw as f64
                }
            };
            if crate::sexp::ffi::is_na_real(value) {
                AtomicUniqueKey::Real(crate::sexp::ffi::R_NA_BIT_PATTERN)
            } else if value.is_nan() {
                AtomicUniqueKey::Real(f64::NAN.to_bits())
            } else if value == 0.0 {
                AtomicUniqueKey::Real(0)
            } else {
                AtomicUniqueKey::Real(value.to_bits())
            }
        }
        _ => {
            let value = if TYPEOF(x) == SEXPTYPE::REALSXP {
                let value = x_owned
                    .try_real_elt(index)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    NA_INTEGER
                } else {
                    value as c_int
                }
            } else {
                integer_like_elt(&x_owned, index)
            };
            AtomicUniqueKey::Integer(value)
        }
    }
}
pub(crate) fn duplicated_key(x: SEXP, index: R_xlen_t, target: SEXPTYPE) -> String {
    {
        if target == SEXPTYPE::REALSXP
            || target == SEXPTYPE::INTSXP
            || target == SEXPTYPE::LGLSXP
            || target == SEXPTYPE::STRSXP
        {
            match atomic_unique_key(x, index, target) {
                AtomicUniqueKey::Integer(v) => format!("i{v}"),
                AtomicUniqueKey::Real(bits) => format!("r{bits}"),
                AtomicUniqueKey::String(s) => format!("s{s}"),
            }
        } else {
            elt_to_string(x, index)
        }
    }
}

pub(crate) fn incomparables_are_none(incomparables: SEXP) -> bool {
    incomparables.is_null()
        || incomparables == R_NilValue()
        || (TYPEOF(incomparables) == SEXPTYPE::LGLSXP && XLENGTH(incomparables) == 1 && {
            let owner = active_owner();
            owner_sexp(owner, incomparables)
                .try_logical_elt(0)
                .unwrap_or(TRUE)
                == FALSE
        })
}

pub(crate) fn atomic_value_is_missing(x: SEXP, index: R_xlen_t) -> bool {
    let owner = active_owner();
    let x = owner_sexp(owner, x);
    match x.typeof_() {
        SEXPTYPE::STRSXP => {
            let elt = x
                .try_string_elt(index)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            charsxp_is_na(elt.as_raw())
        }
        SEXPTYPE::REALSXP => {
            let value = x
                .try_real_elt(index)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || value.is_nan()
        }
        SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => integer_like_elt(&x, index) == NA_INTEGER,
        _ => false,
    }
}

pub(crate) fn copy_atomic_element(
    dst: SEXP,
    dst_index: R_xlen_t,
    src: SEXP,
    src_index: R_xlen_t,
    target_type: SEXPTYPE,
) {
    let owner = active_owner();
    let src_owned = owner_sexp(owner, src);
    let mut dst_mut = owner
        .sexp(dst)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    match target_type {
        t if t == SEXPTYPE::STRSXP => {
            let value = if TYPEOF(src) == SEXPTYPE::STRSXP {
                src_owned
                    .try_string_elt(src_index)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            } else {
                let text = elt_to_string(src, src_index);
                let cstr = CString::new(text).unwrap_or_default();
                let charsxp = mk_char_cstr(&cstr);
                owner
                    .sexp(charsxp)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            };
            dst_mut
                .try_set_string_elt(dst_index, value)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        t if t == SEXPTYPE::REALSXP => {
            let value = if TYPEOF(src) == SEXPTYPE::REALSXP {
                src_owned
                    .try_real_elt(src_index)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            } else {
                let raw = integer_like_elt(&src_owned, src_index);
                if raw == NA_INTEGER {
                    NA_REAL
                } else {
                    raw as f64
                }
            };
            dst_mut
                .try_set_real_elt(dst_index, value)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        _ => {
            let value = if TYPEOF(src) == SEXPTYPE::REALSXP {
                let value = src_owned
                    .try_real_elt(src_index)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    NA_INTEGER
                } else {
                    value as c_int
                }
            } else {
                integer_like_elt(&src_owned, src_index)
            };
            let written = if target_type == SEXPTYPE::LGLSXP {
                dst_mut.try_set_logical_elt(dst_index, value)
            } else {
                dst_mut.try_set_integer_elt(dst_index, value)
            };
            written.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
    }
}

/// R's `sort(x, decreasing, na.last)` — sort an atomic vector.
fn sort_with_index(x: SEXP, decreasing: bool, na_placement: SortNaPlacement) -> SEXP {
    let n = XLENGTH(x) as usize;
    let t = TYPEOF(x);
    #[derive(Clone, Copy)]
    enum Kind {
        Int,
        Real,
        Str,
        Cplx,
        Raw,
    }
    let kind = if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
        Kind::Int
    } else if t == SEXPTYPE::REALSXP {
        Kind::Real
    } else if t == SEXPTYPE::STRSXP {
        Kind::Str
    } else if t == SEXPTYPE::CPLXSXP {
        Kind::Cplx
    } else if t == SEXPTYPE::RAWSXP {
        Kind::Raw
    } else if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "'x' must be atomic".to_string(),
        });
    } else {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "unimplemented type 'unknown' in 'orderVector1'".to_string(),
        });
    };
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    let is_na = |i: usize| -> bool {
        let i = i as R_xlen_t;
        match kind {
            Kind::Int => integer_like_elt(&x_owned, i) == NA_INTEGER,
            Kind::Real => ISNAN(
                x_owned
                    .try_real_elt(i)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
            ),
            Kind::Str => {
                let elt = x_owned
                    .try_string_elt(i)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                charsxp_is_na(elt.as_raw())
            }
            Kind::Cplx => {
                let z = x_owned
                    .try_complex_elt(i)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                ISNAN(z.r) || ISNAN(z.i)
            }
            Kind::Raw => false,
        }
    };
    let cmp = |a: usize, b: usize| -> std::cmp::Ordering {
        let (a, b) = (a as R_xlen_t, b as R_xlen_t);
        let ord = match kind {
            Kind::Int => integer_like_elt(&x_owned, a).cmp(&integer_like_elt(&x_owned, b)),
            Kind::Real => {
                let av = x_owned
                    .try_real_elt(a)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let bv = x_owned
                    .try_real_elt(b)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                av.partial_cmp(&bv).unwrap_or(std::cmp::Ordering::Equal)
            }
            Kind::Str => {
                let ea = x_owned
                    .try_string_elt(a)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let eb = x_owned
                    .try_string_elt(b)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                compare_charsxp_for_sort(ea.as_raw(), eb.as_raw())
            }
            Kind::Cplx => {
                let za = x_owned
                    .try_complex_elt(a)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let zb = x_owned
                    .try_complex_elt(b)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                za.r.partial_cmp(&zb.r)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(za.i.partial_cmp(&zb.i).unwrap_or(std::cmp::Ordering::Equal))
            }
            Kind::Raw => {
                let av = x_owned
                    .try_raw_elt(a)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let bv = x_owned
                    .try_raw_elt(b)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                av.cmp(&bv)
            }
        };
        if decreasing { ord.reverse() } else { ord }
    };
    let mut kept = Vec::new();
    let mut missing = Vec::new();
    for i in 0..n {
        if is_na(i) {
            missing.push(i);
        } else {
            kept.push(i);
        }
    }
    kept.sort_by(|&a, &b| cmp(a, b));
    let mut idx = Vec::new();
    if na_placement == SortNaPlacement::First {
        idx.extend_from_slice(&missing);
    }
    idx.extend_from_slice(&kept);
    if na_placement == SortNaPlacement::Last {
        idx.extend_from_slice(&missing);
    }
    let out_n = idx.len() as i64;
    let values = Rf_allocVector3(t, out_n);
    let _values = protect(values);
    let index = Rf_allocVector3(SEXPTYPE::INTSXP, out_n);
    let _index = protect(index);
    let mut index_mut = owner
        .sexp(index)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let mut values_mut = owner
        .sexp(values)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for (j, &i) in idx.iter().enumerate() {
        index_mut
            .try_set_integer_elt(j as R_xlen_t, (i as i32) + 1)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        match kind {
            Kind::Int => {
                let v = integer_like_elt(&x_owned, i as R_xlen_t);
                let written = if t == SEXPTYPE::LGLSXP {
                    values_mut.try_set_logical_elt(j as R_xlen_t, v)
                } else {
                    values_mut.try_set_integer_elt(j as R_xlen_t, v)
                };
                written.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            Kind::Real => {
                let v = x_owned
                    .try_real_elt(i as R_xlen_t)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                values_mut
                    .try_set_real_elt(j as R_xlen_t, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            Kind::Str => {
                let elt = x_owned
                    .try_string_elt(i as R_xlen_t)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                values_mut
                    .try_set_string_elt(j as R_xlen_t, elt)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            Kind::Cplx => {
                let v = x_owned
                    .try_complex_elt(i as R_xlen_t)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                values_mut
                    .try_set_complex_elt(j as R_xlen_t, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            Kind::Raw => {
                let v = x_owned
                    .try_raw_elt(i as R_xlen_t)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                values_mut
                    .try_set_raw_elt(j as R_xlen_t, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
        }
    }
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    let _result = protect(result);
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let values_owned = owner
        .sexp(values)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let index_owned = owner
        .sexp(index)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    result_mut
        .try_set_vector_elt(0, values_owned)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    result_mut
        .try_set_vector_elt(1, index_owned)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let x_name = mk_char_cstr(c"x");
    let _x_name = protect(x_name);
    let ix_name = mk_char_cstr(c"ix");
    let _ix_name = protect(ix_name);
    let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
    let _names = protect(names);
    if let (Ok(mut names_mut), Ok(x_charsxp), Ok(ix_charsxp)) = (
        owner.sexp(names).and_then(SexpMut::try_from_checked),
        owner.sexp(x_name),
        owner.sexp(ix_name),
    ) {
        let _ = names_mut.try_set_string_elt(0, x_charsxp);
        let _ = names_mut.try_set_string_elt(1, ix_charsxp);
    }
    crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_NamesSymbol(), names);
    result
}

/// R's `sort(x, decreasing, na.last)` — sort an atomic vector.
fn copy_sorted_names(x: SEXP, result: SEXP, order: &[usize], keep: bool) {
    if !keep {
        return;
    }
    let names = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
    if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
        return;
    }
    if XLENGTH(names) != XLENGTH(x) {
        return;
    }
    let out = Rf_allocVector3(SEXPTYPE::STRSXP, order.len() as R_xlen_t);
    if out.is_null() {
        return;
    }
    let _g = protect(out);
    let owner = active_owner();
    let names_owned = owner_sexp(owner, names);
    let mut out_mut = owner
        .sexp(out)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for (dst, src) in order.iter().enumerate() {
        let elt = names_owned
            .try_string_elt(*src as R_xlen_t)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        out_mut
            .try_set_string_elt(dst as R_xlen_t, elt)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_NamesSymbol(), out);
}

pub fn do_sort(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = arg_by_name_or_position(args, &["x"], 0);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    if TYPEOF(x) == SEXPTYPE::RAWSXP {
        base_error("raw vectors cannot be sorted".to_string());
    }
    reject_unorderable(x);
    let decreasing = sort_logical_arg(args, &["decreasing"], 1).unwrap_or(false);
    let na_placement = sort_na_placement(args);
    if sort_logical_arg(args, &["index.return"], 99).unwrap_or(false) {
        return sort_with_index(x, decreasing, na_placement);
    }
    let partial = arg_by_name_or_position(args, &["partial"], usize::MAX);
    let keep_names = partial.is_null() || partial == R_NilValue();
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
        let mut vals: Vec<(i32, usize)> = Vec::with_capacity(n as usize);
        let mut na_idx: Vec<usize> = Vec::new();
        for i in 0..n {
            let value = integer_like_elt(&x_owned, i);
            if value == NA_INTEGER {
                na_idx.push(i as usize);
            } else {
                vals.push((value, i as usize));
            }
        }
        let na_count = na_idx.len();
        if decreasing {
            vals.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        } else {
            vals.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        }
        let output_len = sorted_len(vals.len(), na_count, na_placement);
        let result = Rf_allocVector3(t, output_len as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let mut dst = owner
            .sexp(result)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut order: Vec<usize> = Vec::with_capacity(output_len);
        let mut out = 0usize;
        let put = |dst: &mut SexpMut<'_>, i: usize, value: c_int| {
            let written = if t == SEXPTYPE::LGLSXP {
                dst.try_set_logical_elt(i as R_xlen_t, value)
            } else {
                dst.try_set_integer_elt(i as R_xlen_t, value)
            };
            written.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        };
        if na_placement == SortNaPlacement::First {
            for idx in &na_idx {
                put(&mut dst, out, NA_INTEGER);
                order.push(*idx);
                out += 1;
            }
        }
        for (value, idx) in &vals {
            put(&mut dst, out, *value);
            order.push(*idx);
            out += 1;
        }
        if na_placement == SortNaPlacement::Last {
            for idx in &na_idx {
                put(&mut dst, out, NA_INTEGER);
                order.push(*idx);
                out += 1;
            }
        }
        copy_sorted_names(x, result, &order, keep_names);
        restore_datetime_or_difftime_class(x, result);
        result
    } else if t == SEXPTYPE::REALSXP {
        let mut vals: Vec<(f64, usize)> = Vec::with_capacity(n as usize);
        // GNU radix sort keeps the original payload: NaN and NA both
        // take the ISNAN detour, but the sorted tail (na.last=TRUE) or
        // head (na.last=FALSE) replays them in original order, NaN
        // still printing as NaN.
        let mut na_idx: Vec<(f64, usize)> = Vec::new();
        for i in 0..n {
            let value = x_owned
                .try_real_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            if ISNAN(value) {
                na_idx.push((value, i as usize));
            } else {
                vals.push((value, i as usize));
            }
        }
        let na_count = na_idx.len();
        vals.sort_by(|a, b| {
            let ord = a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal);
            if decreasing { ord.reverse() } else { ord }.then(a.1.cmp(&b.1))
        });
        let output_len = sorted_len(vals.len(), na_count, na_placement);
        let result = Rf_allocVector3(t, output_len as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let mut dst = owner
            .sexp(result)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut order: Vec<usize> = Vec::with_capacity(output_len);
        let mut out = 0usize;
        if na_placement == SortNaPlacement::First {
            for (value, idx) in &na_idx {
                dst.try_set_real_elt(out as R_xlen_t, *value)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                order.push(*idx);
                out += 1;
            }
        }
        for (value, idx) in &vals {
            dst.try_set_real_elt(out as R_xlen_t, *value)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            order.push(*idx);
            out += 1;
        }
        if na_placement == SortNaPlacement::Last {
            for (value, idx) in &na_idx {
                dst.try_set_real_elt(out as R_xlen_t, *value)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                order.push(*idx);
                out += 1;
            }
        }
        copy_sorted_names(x, result, &order, keep_names);
        restore_datetime_or_difftime_class(x, result);

        result
    } else if t == SEXPTYPE::STRSXP {
        let mut vals: Vec<SEXP> = Vec::with_capacity(n as usize);
        let mut na_count = 0usize;
        for i in 0..n {
            let value = x_owned
                .try_string_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                .as_raw();
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
        let mut dst = owner
            .sexp(result)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let na_string = owner
            .sexp(crate::sexp::globals::R_NaString())
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut out = 0usize;
        if na_placement == SortNaPlacement::First {
            for _ in 0..na_count {
                dst.try_set_string_elt(out as R_xlen_t, na_string.clone())
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                out += 1;
            }
        }
        for value in vals {
            let value = owner
                .sexp(value)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            dst.try_set_string_elt(out as R_xlen_t, value)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            out += 1;
        }
        if na_placement == SortNaPlacement::Last {
            for _ in 0..na_count {
                dst.try_set_string_elt(out as R_xlen_t, na_string.clone())
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                out += 1;
            }
        }
        restore_datetime_or_difftime_class(x, result);
        result
    } else if t == SEXPTYPE::RAWSXP {
        let mut vals: Vec<u8> = Vec::with_capacity(n as usize);
        for i in 0..n {
            vals.push(
                x_owned
                    .try_raw_elt(i)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
            );
        }
        vals.sort_unstable();
        if decreasing {
            vals.reverse();
        }
        let result = Rf_allocVector3(t, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let mut dst = owner
            .sexp(result)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        for (i, byte) in vals.iter().enumerate() {
            dst.try_set_raw_elt(i as R_xlen_t, *byte)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if !class.is_null() && class != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }
        result
    } else {
        let result = Rf_allocVector3(t, n);
        if result.is_null() {
            return R_NilValue();
        }
        restore_datetime_or_difftime_class(x, result);
        result
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum SortNaPlacement {
    Remove,
    Last,
    First,
    Keep,
}

fn sorted_len(value_count: usize, na_count: usize, na_placement: SortNaPlacement) -> usize {
    value_count
        + match na_placement {
            SortNaPlacement::Remove => 0,
            SortNaPlacement::Last | SortNaPlacement::First | SortNaPlacement::Keep => na_count,
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
    let arg = arg_by_name_or_position(args, names, position);
    if arg.is_null() || arg == R_NilValue() || XLENGTH(arg) == 0 {
        return None;
    }
    let owner = active_owner();
    let arg_owned = owner_sexp(owner, arg);
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
        return None;
    };
    (raw != NA_LOGICAL).then_some(raw != 0)
}

pub(crate) fn charsxp_is_na(value: SEXP) -> bool {
    value.is_null() || value == crate::sexp::globals::R_NaString()
}

pub(crate) fn collate_str(a: &str, b: &str) -> std::cmp::Ordering {
    // GNU sortVector / Scollate: strcoll under the current LC_COLLATE.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let ac = std::ffi::CString::new(a).unwrap_or_default();
        let bc = std::ffi::CString::new(b).unwrap_or_default();
        return match unsafe { libc::strcoll(ac.as_ptr(), bc.as_ptr()) } {
            n if n < 0 => std::cmp::Ordering::Less,
            n if n > 0 => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        };
    }
    #[cfg(target_arch = "wasm32")]
    {
        a.cmp(b)
    }
}

pub(crate) fn compare_charsxp_for_sort(a: SEXP, b: SEXP) -> std::cmp::Ordering {
    let a_is_na = charsxp_is_na(a);
    let b_is_na = charsxp_is_na(b);
    match (a_is_na, b_is_na) {
        (true, true) => return std::cmp::Ordering::Equal,
        (true, false) => return std::cmp::Ordering::Greater,
        (false, true) => return std::cmp::Ordering::Less,
        (false, false) => {}
    }
    let owner = active_owner();
    let text = |value: SEXP| -> String {
        let owned = owner_sexp(owner, value);
        owned
            .header()
            .char_bytes()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_default()
    };
    collate_str(&text(a), &text(b))
}

/// R's `rev(x)` — reverse a vector.
pub fn do_rev(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for i in 0..n {
        let src = (n - 1 - i) as R_xlen_t;
        let dst = i as R_xlen_t;
        if t == SEXPTYPE::REALSXP {
            let v = x_owned
                .try_real_elt(src)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            result_mut
                .try_set_real_elt(dst, v)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = integer_like_elt(&x_owned, src);
            let written = if t == SEXPTYPE::LGLSXP {
                result_mut.try_set_logical_elt(dst, v)
            } else {
                result_mut.try_set_integer_elt(dst, v)
            };
            written.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        } else if t == SEXPTYPE::STRSXP {
            let elt = x_owned
                .try_string_elt(src)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            result_mut
                .try_set_string_elt(dst, elt)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        } else if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP {
            let elt = x_owned
                .try_vector_elt(src)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            result_mut
                .try_set_vector_elt(dst, elt)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        } else if t == SEXPTYPE::RAWSXP {
            let v = x_owned
                .try_raw_elt(src)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            result_mut
                .try_set_raw_elt(dst, v)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
    }
    reverse_names_attribute(x, result, n);
    restore_datetime_or_difftime_class(x, result);
    result
}

fn reverse_names_attribute(x: SEXP, result: SEXP, len: R_xlen_t) {
    let names = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
    if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
        return;
    }
    let reversed = Rf_allocVector3(SEXPTYPE::STRSXP, len);
    if reversed.is_null() {
        return;
    }
    let _reversed_guard = protect(reversed);
    let owner = active_owner();
    let names_owned = owner_sexp(owner, names);
    let mut reversed_mut = owner
        .sexp(reversed)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for i in 0..len {
        let src = len - 1 - i;
        let elt = names_owned
            .try_string_elt(src)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        reversed_mut
            .try_set_string_elt(i, elt)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_NamesSymbol(),
        reversed,
    );
}

/// GNU Boolean Summary default with original-owner roots and admitted coercion.
pub fn do_any(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    super::boolean_summary::run(call, op, args, rho, true)
}

pub fn do_all(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    super::boolean_summary::run(call, op, args, rho, false)
}

/// R's `cumsum(x)` — cumulative sum.
pub fn do_cumsum(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut dispatched = R_NilValue();
        if crate::eval::dispatch::DispatchGroup(
            c"Math".as_ptr(),
            call,
            op,
            args,
            rho,
            &mut dispatched,
        ) != 0
        {
            return dispatched;
        }
        crate::mainutils::cum::do_cumsum(call, op, args, rho)
    }
}

/// R's `cumprod(x)` — cumulative product.
pub fn do_cumprod(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut dispatched = R_NilValue();
        if crate::eval::dispatch::DispatchGroup(
            c"Math".as_ptr(),
            call,
            op,
            args,
            rho,
            &mut dispatched,
        ) != 0
        {
            return dispatched;
        }
        crate::mainutils::cum::do_cumprod(call, op, args, rho)
    }
}

/// R's `cumvar(x)` — cumulative sample variance by Youngs-Cramer algorithm.
pub fn do_cumvar(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut dispatched = R_NilValue();
        if crate::eval::dispatch::DispatchGroup(
            c"Math".as_ptr(),
            call,
            op,
            args,
            rho,
            &mut dispatched,
        ) != 0
        {
            return dispatched;
        }
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
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol()),
        );
        let owner = active_owner();
        let mut dst = owner
            .sexp(result)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if n == 0 {
            return result;
        }

        dst.try_set_real_elt(0, NA_REAL)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut var = 0.0f64;
        let mut sum = elt_real_safe(x, 0);
        let mut poisoned = sum.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || sum.is_nan();
        for i in 1..n {
            let value = elt_real_safe(x, i);
            if poisoned || value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || value.is_nan() {
                poisoned = true;
                dst.try_set_real_elt(i, NA_REAL)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                continue;
            }
            sum += value;
            let count = (i + 1) as f64;
            let numerator = count * value - sum;
            var += numerator.powi(2) / (i as f64 * count);
            dst.try_set_real_elt(i, var / i as f64)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        result
    }
}

/// R's `diff(x, lag)` — lagged differences.
pub fn do_diff(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let lag_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    if sexp_has_class(x, "Date") {
        return do_diff_Date(_call, _op, args, _rho);
    }
    if sexp_has_class(x, "POSIXlt") || sexp_has_class(x, "POSIXct") || sexp_has_class(x, "POSIXt") {
        return do_diff_POSIXt(_call, _op, args, _rho);
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

    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    let mut dst = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    if result_type == SEXPTYPE::INTSXP {
        let mut warned = false;
        for i in 0..result_len {
            let a = integer_like_elt(&x_owned, i);
            let b = integer_like_elt(&x_owned, i + lag as R_xlen_t);
            let value = if a == NA_INTEGER || b == NA_INTEGER {
                NA_INTEGER
            } else {
                let diff = b as i64 - a as i64;
                if diff > i32::MAX as i64 || diff < i32::MIN as i64 {
                    if !warned {
                        warned = true;
                        let msg = c"NAs produced by integer overflow";
                        crate::mainutils::errors::Rf_warning_cstr(&msg);
                    }
                    NA_INTEGER
                } else {
                    diff as c_int
                }
            };
            dst.try_set_integer_elt(i, value)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        return result;
    }

    for i in 0..result_len {
        let a = elt_real_safe(x, i);
        let b = elt_real_safe(x, i + lag as R_xlen_t);
        dst.try_set_real_elt(i, b - a)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    result
}
