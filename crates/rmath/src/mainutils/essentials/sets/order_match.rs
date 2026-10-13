use super::*;

pub(crate) unsafe fn match_arg(args: SEXP, position: usize, name: &str, default: SEXP) -> SEXP {
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
            let sym = Rf_install(c"cut.dendrogram".as_ptr());
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

unsafe fn unique_matrix_rows(x: SEXP, sexptype: SEXPTYPE) -> Option<SEXP> {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null()
            || dim == R_NilValue()
            || TYPEOF(dim) != SEXPTYPE::INTSXP
            || XLENGTH(dim) != 2
        {
            return None;
        }
        let nrow = *INTEGER(dim) as R_xlen_t;
        let ncol = *INTEGER(dim).add(1) as R_xlen_t;
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
                match TYPEOF(x) {
                    t if t == SEXPTYPE::STRSXP => {
                        SET_STRING_ELT(result, dst, STRING_ELT(x, src));
                    }
                    t if t == SEXPTYPE::REALSXP => {
                        *REAL(result).add(dst as usize) = *REAL(x).add(src as usize);
                    }
                    _ => {
                        *INTEGER(result).add(dst as usize) = *INTEGER(x).add(src as usize);
                    }
                }
            }
        }
        let out_dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        let _d = protect(out_dim);
        *INTEGER(out_dim) = nkeep as c_int;
        *INTEGER(out_dim).add(1) = ncol as c_int;
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimSymbol(),
            out_dim,
        );
        Some(result)
    }
}

/// R's `unique(x)` — return unique atomic elements in R's retained-index order.
pub unsafe fn do_unique(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
                for i in 0..n {
                    if *LOGICAL(dup).add(i as usize) == FALSE {
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
            for (j, &i) in keep.iter().enumerate() {
                SET_VECTOR_ELT(result, j as R_xlen_t, VECTOR_ELT(x, i));
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
        restore_datetime_or_difftime_class(x, result);
        result
    }
}

pub(crate) unsafe fn restore_datetime_or_difftime_class(source: SEXP, result: SEXP) {
    unsafe {
        if result.is_null() || result == R_NilValue() {
            return;
        }
        if sexp_has_class(source, "Date") {
            set_datetime_class_from(result, source, DatetimeVectorClass::Date);
        } else if sexp_has_class(source, "POSIXct") || sexp_has_class(source, "POSIXt") {
            set_datetime_class_from(result, source, DatetimeVectorClass::Posixct);
        } else if sexp_has_class(source, "difftime") {
            let units = crate::sexp::attrib_core::getAttrib(
                source,
                crate::sexp::symbol::Rf_install(c"units".as_ptr()),
            );
            set_single_class(result, "difftime");
            if !units.is_null() && units != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::symbol::Rf_install(c"units".as_ptr()),
                    units,
                );
            }
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
    unsafe {
        let mut keys = std::collections::BTreeSet::new();
        if incomparables.is_null() || incomparables == R_NilValue() {
            return keys;
        }
        // GNU: `incomparables = FALSE` means every value can be compared.
        if TYPEOF(incomparables) == SEXPTYPE::LGLSXP
            && XLENGTH(incomparables) == 1
            && *LOGICAL(incomparables) == FALSE
        {
            return keys;
        }
        let n = XLENGTH(incomparables);
        for i in 0..n {
            keys.insert(atomic_unique_key(incomparables, i, target_type));
        }
        keys
    }
}

pub(crate) fn atomic_unique_key(
    x: SEXP,
    index: R_xlen_t,
    target_type: SEXPTYPE,
) -> AtomicUniqueKey {
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
pub(crate) fn duplicated_key(x: SEXP, index: R_xlen_t, target: SEXPTYPE) -> String {
    unsafe {
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
    unsafe {
        incomparables.is_null()
            || incomparables == R_NilValue()
            || (TYPEOF(incomparables) == SEXPTYPE::LGLSXP
                && XLENGTH(incomparables) == 1
                && *LOGICAL(incomparables) == FALSE)
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

pub(crate) fn copy_atomic_element(
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
unsafe fn sort_with_index(x: SEXP, decreasing: bool, na_placement: SortNaPlacement) -> SEXP {
    unsafe {
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
        let is_na = |i: usize| -> bool {
            match kind {
                Kind::Int => *INTEGER(x).add(i) == NA_INTEGER,
                Kind::Real => ISNAN(*REAL(x).add(i)),
                Kind::Str => charsxp_is_na(STRING_ELT(x, i as R_xlen_t)),
                Kind::Cplx => {
                    let z = *crate::sexp::accessors::COMPLEX(x).add(i);
                    ISNAN(z.r) || ISNAN(z.i)
                }
                Kind::Raw => false,
            }
        };
        let cmp = |a: usize, b: usize| -> std::cmp::Ordering {
            let ord = match kind {
                Kind::Int => (*INTEGER(x).add(a)).cmp(&*INTEGER(x).add(b)),
                Kind::Real => (*REAL(x).add(a))
                    .partial_cmp(&*REAL(x).add(b))
                    .unwrap_or(std::cmp::Ordering::Equal),
                Kind::Str => compare_charsxp_for_sort(
                    STRING_ELT(x, a as R_xlen_t),
                    STRING_ELT(x, b as R_xlen_t),
                ),
                Kind::Cplx => {
                    let za = *crate::sexp::accessors::COMPLEX(x).add(a);
                    let zb = *crate::sexp::accessors::COMPLEX(x).add(b);
                    za.r.partial_cmp(&zb.r)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(za.i.partial_cmp(&zb.i).unwrap_or(std::cmp::Ordering::Equal))
                }
                Kind::Raw => (*RAW(x).add(a)).cmp(&*RAW(x).add(b)),
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
        for (j, &i) in idx.iter().enumerate() {
            *INTEGER(index).add(j) = (i as i32) + 1;
            match kind {
                Kind::Int => *INTEGER(values).add(j) = *INTEGER(x).add(i),
                Kind::Real => *REAL(values).add(j) = *REAL(x).add(i),
                Kind::Str => SET_STRING_ELT(values, j as R_xlen_t, STRING_ELT(x, i as R_xlen_t)),
                Kind::Cplx => {
                    *crate::sexp::accessors::COMPLEX(values).add(j) =
                        *crate::sexp::accessors::COMPLEX(x).add(i);
                }
                Kind::Raw => *RAW(values).add(j) = *RAW(x).add(i),
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _result = protect(result);
        SET_VECTOR_ELT(result, 0, values);
        SET_VECTOR_ELT(result, 1, index);
        let x_name = Rf_mkChar(c"x".as_ptr());
        let _x_name = protect(x_name);
        let ix_name = Rf_mkChar(c"ix".as_ptr());
        let _ix_name = protect(ix_name);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _names = protect(names);
        SET_STRING_ELT(names, 0, x_name);
        SET_STRING_ELT(names, 1, ix_name);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        result
    }
}

/// R's `sort(x, decreasing, na.last)` — sort an atomic vector.
unsafe fn copy_sorted_names(x: SEXP, result: SEXP, order: &[usize], keep: bool) {
    unsafe {
        if !keep {
            return;
        }
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
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
        for (dst, src) in order.iter().enumerate() {
            SET_STRING_ELT(out, dst as R_xlen_t, STRING_ELT(names, *src as R_xlen_t));
        }
        crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_NamesSymbol(), out);
    }
}

pub unsafe fn do_sort(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
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
        if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let mut vals: Vec<(i32, usize)> = Vec::with_capacity(n as usize);
            let mut na_idx: Vec<usize> = Vec::new();
            for i in 0..n {
                let value = *INTEGER(x).add(i as usize);
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
            let dst = INTEGER(result);
            let mut order: Vec<usize> = Vec::with_capacity(output_len);
            let mut out = 0usize;
            if na_placement == SortNaPlacement::First {
                for idx in &na_idx {
                    *dst.add(out) = NA_INTEGER;
                    order.push(*idx);
                    out += 1;
                }
            }
            for (value, idx) in &vals {
                *dst.add(out) = *value;
                order.push(*idx);
                out += 1;
            }
            if na_placement == SortNaPlacement::Last {
                for idx in &na_idx {
                    *dst.add(out) = NA_INTEGER;
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
                let value = *REAL(x).add(i as usize);
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
            let dst = REAL(result);
            let mut order: Vec<usize> = Vec::with_capacity(output_len);
            let mut out = 0usize;
            if na_placement == SortNaPlacement::First {
                for (value, idx) in &na_idx {
                    *dst.add(out) = *value;
                    order.push(*idx);
                    out += 1;
                }
            }
            for (value, idx) in &vals {
                *dst.add(out) = *value;
                order.push(*idx);
                out += 1;
            }
            if na_placement == SortNaPlacement::Last {
                for (value, idx) in &na_idx {
                    *dst.add(out) = *value;
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
            restore_datetime_or_difftime_class(x, result);
            result
        } else if t == SEXPTYPE::RAWSXP {
            let mut vals: Vec<u8> = Vec::with_capacity(n as usize);
            for i in 0..n {
                vals.push(*RAW(x).add(i as usize));
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
            for (i, byte) in vals.iter().enumerate() {
                *RAW(result).add(i) = *byte;
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
        collate_str(a_text, b_text)
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
        restore_datetime_or_difftime_class(x, result);
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
            t if t == SEXPTYPE::STRSXP.as_c_int() => {
                let elt = STRING_ELT(x, index);
                if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                    Some(NA_INTEGER)
                } else {
                    let text = std::ffi::CStr::from_ptr(CHAR(elt)).to_string_lossy();
                    match text.as_ref() {
                        "T" | "TRUE" => Some(TRUE),
                        "F" | "FALSE" => Some(FALSE),
                        _ => Some(NA_INTEGER),
                    }
                }
            }
            _ => {
                let kind = match TYPEOF(x) {
                    t if t == SEXPTYPE::SYMSXP.as_c_int() => "symbol",
                    t if t == SEXPTYPE::LANGSXP.as_c_int() => "language",
                    t if t == SEXPTYPE::CLOSXP.as_c_int() => "closure",
                    t if t == SEXPTYPE::ENVSXP.as_c_int() => "environment",
                    _ => "unknown",
                };
                base_error(format!(
                    "'{kind}' object cannot be coerced to type 'logical'"
                ));
            }
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
pub unsafe fn do_any(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut dispatched = R_NilValue();
        if crate::eval::dispatch::DispatchGroup(
            c"Summary".as_ptr(),
            call,
            op,
            args,
            rho,
            &mut dispatched,
        ) != 0
        {
            return dispatched;
        }
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
                        Some(v) if v != FALSE && v != NA_INTEGER => {
                            return Rf_ScalarLogical(TRUE);
                        }
                        Some(NA_INTEGER) if !na_rm => has_na = true,
                        _ => {}
                    }
                }
            }
            current = CDR(current);
        }

        Rf_ScalarLogical(if has_na { NA_INTEGER } else { FALSE })
    }
}

/// R's `all(...)` — TRUE if all elements are TRUE.
pub unsafe fn do_all(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut dispatched = R_NilValue();
        if crate::eval::dispatch::DispatchGroup(
            c"Summary".as_ptr(),
            call,
            op,
            args,
            rho,
            &mut dispatched,
        ) != 0
        {
            return dispatched;
        }
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
                        _ => {}
                    }
                }
            }
            current = CDR(current);
        }

        Rf_ScalarLogical(if has_na { NA_INTEGER } else { TRUE })
    }
}

/// R's `cumsum(x)` — cumulative sum.
pub unsafe fn do_cumsum(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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
pub unsafe fn do_cumprod(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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
pub unsafe fn do_cumvar(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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
        let dst = REAL(result);
        if n == 0 {
            return result;
        }

        *dst = NA_REAL;
        let mut var = 0.0f64;
        let mut sum = elt_real_safe(x, 0);
        let mut poisoned = sum.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || sum.is_nan();
        for i in 1..n {
            let value = elt_real_safe(x, i);
            if poisoned || value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || value.is_nan() {
                poisoned = true;
                *dst.add(i as usize) = NA_REAL;
                continue;
            }
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
        if sexp_has_class(x, "Date") {
            return do_diff_Date(_call, _op, args, _rho);
        }
        if sexp_has_class(x, "POSIXlt")
            || sexp_has_class(x, "POSIXct")
            || sexp_has_class(x, "POSIXt")
        {
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
                            let msg = c"NAs produced by integer overflow";
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
