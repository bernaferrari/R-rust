use super::*;

// ---------------------------------------------------------------------------
// do_subset -- the `[` subset operator
// ---------------------------------------------------------------------------

/// The `[` subset operator -- the most general form of subsetting.
unsafe fn posixlt_extract_component(x: SEXP, j: SEXP) -> SEXP {
    unsafe {
        let ncomp = XLENGTH(x);
        if TYPEOF(j) == SEXPTYPE::STRSXP && XLENGTH(j) > 0 {
            let ch = STRING_ELT(j, 0);
            if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
                errorcall(std::ptr::null_mut(), "invalid POSIXlt component");
            }
            let name = std::ffi::CStr::from_ptr(CHAR(ch)).to_string_lossy();
            let names = getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            if !isNull(names) && TYPEOF(names) == SEXPTYPE::STRSXP {
                for i in 0..XLENGTH(names).min(ncomp) {
                    let ni = STRING_ELT(names, i);
                    if ni.is_null() {
                        continue;
                    }
                    let nm = std::ffi::CStr::from_ptr(CHAR(ni)).to_string_lossy();
                    if nm == name {
                        return VECTOR_ELT(x, i);
                    }
                }
            }
            for (i, nm) in crate::mainutils::datetime::ltnames.iter().enumerate() {
                if *nm == name && (i as R_xlen_t) < ncomp {
                    return VECTOR_ELT(x, i as R_xlen_t);
                }
            }
            errorcall(std::ptr::null_mut(), "invalid POSIXlt component");
        }
        let idx = if TYPEOF(j) == SEXPTYPE::INTSXP && XLENGTH(j) > 0 {
            *INTEGER(j) as R_xlen_t
        } else if TYPEOF(j) == SEXPTYPE::REALSXP && XLENGTH(j) > 0 {
            *REAL(j) as R_xlen_t
        } else {
            0
        };
        if idx < 1 || idx > ncomp {
            errorcall(std::ptr::null_mut(), "invalid POSIXlt component");
        }
        VECTOR_ELT(x, idx - 1)
    }
}

pub(crate) unsafe fn posixlt_index_preserves_balance(
    x: SEXP,
    i: SEXP,
    n: R_xlen_t,
    call: SEXP,
    op: SEXP,
    env: SEXP,
) -> bool {
    unsafe {
        let bal = getAttrib(x, Rf_install(c"balanced".as_ptr()));
        if bal.is_null()
            || bal == R_NilValue()
            || TYPEOF(bal) != SEXPTYPE::LGLSXP
            || XLENGTH(bal) == 0
            || *INTEGER(bal) != TRUE
        {
            return false;
        }
        if i.is_null() || i == R_NilValue() || i == R_MissingArg() {
            return true;
        }
        let seq = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        let _s = protect(seq);
        for k in 0..n {
            *INTEGER(seq).add(k as usize) = (k + 1) as i32;
        }
        let args = Rf_cons(seq, Rf_cons(i, R_NilValue()));
        let _g = protect(args);
        let sub = do_subset_dflt(call, op, args, env);
        let _u = protect(sub);
        if sub.is_null() || sub == R_NilValue() {
            return true;
        }
        if TYPEOF(sub) == SEXPTYPE::INTSXP {
            for k in 0..XLENGTH(sub) {
                if *INTEGER(sub).add(k as usize) == NA_INTEGER {
                    return false;
                }
            }
            return true;
        }
        if TYPEOF(sub) == SEXPTYPE::REALSXP {
            for k in 0..XLENGTH(sub) {
                let v = *REAL(sub).add(k as usize);
                if v.is_nan() {
                    return false;
                }
            }
            return true;
        }
        false
    }
}

unsafe fn subset_posixlt_time(x: SEXP, i: SEXP, call: SEXP, op: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let ncomp = XLENGTH(x);
        let mut n: R_xlen_t = 0;
        for j in 0..ncomp {
            let len = XLENGTH(VECTOR_ELT(x, j));
            if len > n {
                n = len;
            }
        }
        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, ncomp);
        let _a = protect(ans);
        for j in 0..ncomp {
            let col = crate::mainutils::datetime::recycle_posixlt_component(VECTOR_ELT(x, j), n);
            let _c = protect(col);
            let args = Rf_cons(col, Rf_cons(i, R_NilValue()));
            let _g = protect(args);
            let sub = do_subset_dflt(call, op, args, env);
            SET_VECTOR_ELT(ans, j, sub);
        }
        let names = getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if !isNull(names) {
            setAttrib(ans, crate::sexp::attrib_core::R_NamesSymbol(), names);
        }
        setAttrib(ans, sym_Class(), getAttrib(x, sym_Class()));
        let tzone = getAttrib(x, sym_Tzone());
        if !isNull(tzone) {
            setAttrib(ans, sym_Tzone(), tzone);
        }
        // GNU `[.POSIXlt`: balanced stays TRUE iff x is balanced and
        // seq_along(x)[i] has no NA (out-of-range / missing indices).
        let keep_balanced = posixlt_index_preserves_balance(x, i, n, call, op, env);
        setAttrib(
            ans,
            Rf_install(c"balanced".as_ptr()),
            Rf_ScalarLogical(if keep_balanced { TRUE } else { NA_LOGICAL }),
        );
        ans
    }
}

pub(crate) unsafe fn subset_posixlt_obs(x: SEXP, i: SEXP, call: SEXP, op: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut idx = i;
        if TYPEOF(i) == SEXPTYPE::STRSXP && XLENGTH(i) > 0 {
            let year = if XLENGTH(x) > 5 {
                VECTOR_ELT(x, 5)
            } else {
                R_NilValue()
            };
            let names = if !year.is_null() && year != R_NilValue() {
                getAttrib(year, crate::sexp::attrib_core::R_NamesSymbol())
            } else {
                R_NilValue()
            };
            let ch = STRING_ELT(i, 0);
            let needle = if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned()
            };
            let mut found = 0i32;
            if !isNull(names) && TYPEOF(names) == SEXPTYPE::STRSXP && !needle.is_empty() {
                for k in 0..XLENGTH(names) {
                    let nk = STRING_ELT(names, k);
                    if nk.is_null() || nk == crate::sexp::globals::R_NaString() {
                        continue;
                    }
                    let nm = std::ffi::CStr::from_ptr(CHAR(nk)).to_string_lossy();
                    if nm == needle {
                        found = (k + 1) as i32;
                        break;
                    }
                }
            }
            if found == 0 {
                errorcall(
                    call,
                    &format!(
                        "No element named \"{needle}\" found in x, did you mean x[, \"{needle}\"] instead?"
                    ),
                );
            }

            idx = Rf_ScalarInteger(found);
        }
        let _i = protect(idx);
        subset_posixlt_time(x, idx, call, op, env)
    }
}

pub unsafe fn do_subset(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();

        /* DispatchOrEval internal generic: [ */
        if R_DispatchOrEvalSP(
            call,
            op,
            b"[\0".as_ptr() as *const c_char,
            args,
            env,
            &mut ans,
        ) != 0
        {
            if NAMED(ans) != 0 {
                ENSURE_NAMEDMAX(ans);
            }
            return ans;
        }

        /* Method dispatch has failed, we now run the generic internal code
         * plus the base S3 `[.factor` fixup in one shared tail. */
        let orig = CAR(ans);
        if crate::mainutils::essentials::sexp_has_class(orig, "POSIXlt")
            && TYPEOF(orig) == SEXPTYPE::VECSXP
        {
            let idx = CADR(ans);
            let rest = CDDR(ans);
            let j = if !rest.is_null() && rest != R_NilValue() && TAG(rest) != sym_Drop() {
                CAR(rest)
            } else {
                R_NilValue()
            };
            let i_missing = idx.is_null() || idx == R_NilValue() || idx == R_MissingArg();
            let j_missing = j.is_null() || j == R_NilValue() || j == R_MissingArg();
            if i_missing && !j_missing {
                return posixlt_extract_component(orig, j);
            }
            let only_time_index = !i_missing
                && (rest.is_null() || rest == R_NilValue() || TAG(rest) == sym_Drop() || j_missing);
            if only_time_index {
                return subset_posixlt_time(orig, idx, call, op, env);
            }
        }

        subset_default_after_failed_dispatch(call, op, ans, env)
    }
}

/// The default `[` tail after S3 dispatch failed: `do_subset_dflt` plus the
/// base S3 `[.factor` fixup. Stock R dispatches factor subsetting to the
/// `[.factor` method; this port carries no `[.factor` closure in base, so
/// the method's observable behavior is reproduced here. The AST `[` builtin
/// (`do_subset`) and the compiled DFLTSUBSET / MATSUBSET / SUBSET_N default
/// paths share this tail, keeping interpreted and compiled subsetting
/// identical.
///
/// `ans` is the evaluated argument list with `x` first; a tagged `drop`
/// argument is honored (it matches the `[.factor` formal whose default is
/// `drop = FALSE`).
pub unsafe fn subset_default_after_failed_dispatch(
    call: SEXP,
    op: SEXP,
    ans: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        // Peek `drop` first: do_subset_dflt's ExtractDropArg REMOVES it
        // from the evaluated argument list. The internal `[` default is
        // drop = TRUE, but the fixup below emulates the S3 `[.factor`
        // whose formal default is drop = FALSE.
        let mut drop: c_int = 0;
        {
            let mut cell = ans;
            while !isNull(cell) {
                if TAG(cell) == sym_Drop() {
                    drop = asLogical(CAR(cell));
                    if drop == NA_LOGICAL && has_class_factor(CAR(ans)) {
                        // `if (drop)` with NA in upstream [.factor.
                        errorcall(call, "missing value where TRUE/FALSE needed");
                    }
                    break;
                }
                cell = CDR(cell);
            }
        }
        let result = do_subset_dflt(call, op, ans, env);
        // Base R ships an S3 `[.factor`:
        //   y <- NextMethod("[")
        //   attr(y, "contrasts") <- attr(x, "contrasts")
        //   attr(y, "levels") <- attr(x, "levels")
        //   class(y) <- oldClass(x)
        //   if (drop) factor(y, exclude = if (anyNA(levels(x))) NULL else NA) else y
        // The engine's default `[` strips attributes like stock's
        // NextMethod target, so apply the same fixup here: subsetting a
        // factor yields a factor with the original levels, contrasts, and
        // class; with drop = TRUE unused levels are dropped (the factor()
        // rebuild keeps used levels in code order, remaps the codes, and
        // drops the contrasts attribute).
        let orig = CAR(ans);
        if has_class_factor(orig) {
            let _r_guard = protect(result);
            let _o_guard = protect(orig);
            let levels = getAttrib(orig, crate::eval::attrib_core::R_LevelsSymbol());
            if !isNull(levels) {
                setAttrib(result, crate::eval::attrib_core::R_LevelsSymbol(), levels);
            }
            let class_attr = getAttrib(orig, sym_Class());
            if !isNull(class_attr) {
                setAttrib(result, sym_Class(), class_attr);
            }
            if drop == 0 {
                // drop = FALSE: contrasts travel with the levels like
                // every other kept attribute.
                let contrasts = getAttrib(orig, sym_Contrasts());
                if !isNull(contrasts) {
                    setAttrib(result, sym_Contrasts(), contrasts);
                }
            } else if TYPEOF(result) == SEXPTYPE::INTSXP && !isNull(levels) {
                drop_unused_factor_levels(result, levels);
            }
            return result;
        }
        result
    }
}

pub unsafe fn factor_replacement_codes(target: SEXP, value: SEXP, call: SEXP) -> SEXP {
    unsafe {
        if !has_class_factor(target)
            || TYPEOF(target) != SEXPTYPE::INTSXP
            || TYPEOF(value) != SEXPTYPE::STRSXP
        {
            return value;
        }
        let levels = getAttrib(target, crate::eval::attrib_core::R_LevelsSymbol());
        if isNull(levels) || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return value;
        }
        let nv = XLENGTH(value);
        let codes = Rf_allocVector3(SEXPTYPE::INTSXP, nv);
        let _cg = protect(codes);
        for i in 0..nv {
            let vs = crate::sexp::accessors::STRING_ELT(value, i);
            let mut code: c_int = NA_INTEGER;
            if !vs.is_null() && vs != crate::sexp::globals::R_NaString() {
                let vb = crate::sexp::accessors::CHAR(vs);
                for (j, lev) in (0..XLENGTH(levels)).enumerate() {
                    let ls = crate::sexp::accessors::STRING_ELT(levels, lev);
                    if !ls.is_null()
                        && ls != crate::sexp::globals::R_NaString()
                        && std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(ls)).to_bytes()
                            == std::ffi::CStr::from_ptr(vb).to_bytes()
                    {
                        code = (j + 1) as c_int;
                        break;
                    }
                }
            }
            *crate::sexp::accessors::INTEGER(codes).add(i as usize) = code;
        }
        let _ = call;
        codes
    }
}

/// The `drop = TRUE` tail of upstream `[.factor`:
/// `factor(y, exclude = if (anyNA(levels(x))) NULL else NA)`.
///
/// Keeps the levels present in the (already levels/class-attributed) code
/// vector `result`, in original level order, remapping the integer codes
/// to the shrunken level set; NA codes stay NA. Other attributes set by
/// the fixup survive except contrasts, which the upstream factor()
/// rebuild drops.
unsafe fn drop_unused_factor_levels(result: SEXP, levels: SEXP) {
    unsafe {
        let n_levels = XLENGTH(levels);
        // Which level codes appear in the subset (1-based)?
        let mut used = vec![false; n_levels as usize];
        let n = XLENGTH(result);
        let codes = INTEGER(result);
        for i in 0..n as usize {
            let code = *codes.add(i);
            if code != NA_INTEGER && code >= 1 && (code as usize) <= n_levels as usize {
                used[(code - 1) as usize] = true;
            }
        }
        let new_index_of: Vec<i32> = std::iter::once(0)
            .chain(used.iter().scan(0i32, |acc, &u| {
                if u {
                    *acc += 1;
                }
                Some(*acc)
            }))
            .collect();
        let kept: usize = used.iter().filter(|&&u| u).count();
        if kept == n_levels as usize {
            // Every level is still used: factor() is a no-op beyond
            // dropping the contrasts attribute.
            setAttrib(result, sym_Contrasts(), R_NilValue());
            return;
        }
        let new_levels = Rf_allocVector(SEXPTYPE::STRSXP, kept as c_int);
        let _nl_guard = protect(new_levels);
        let mut slot = 0usize;
        for (i, &u) in used.iter().enumerate() {
            if u {
                SET_STRING_ELT(
                    new_levels,
                    slot as i64,
                    crate::sexp::accessors::STRING_ELT(levels, i as i64),
                );
                slot += 1;
            }
        }
        for i in 0..n as usize {
            let code = *codes.add(i);
            *codes.add(i) = if code == NA_INTEGER || code < 1 || (code as usize) > n_levels as usize
            {
                NA_INTEGER
            } else {
                new_index_of[code as usize]
            };
        }
        setAttrib(
            result,
            crate::eval::attrib_core::R_LevelsSymbol(),
            new_levels,
        );
        setAttrib(result, sym_Contrasts(), R_NilValue());
    }
}

pub(crate) unsafe fn has_class_factor(x: SEXP) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        let class_attr = getAttrib(x, sym_Class());
        if isNull(class_attr) || TYPEOF(class_attr) != SEXPTYPE::STRSXP {
            return false;
        }
        (0..length_int(class_attr)).any(|i| {
            let si = crate::sexp::accessors::STRING_ELT(class_attr, i as i64);
            if si.is_null() || si == crate::sexp::globals::R_NaString() {
                return false;
            }
            let c = crate::sexp::accessors::CHAR(si);
            !c.is_null() && std::ffi::CStr::from_ptr(c).to_bytes() == b"factor"
        })
    }
}

// ---------------------------------------------------------------------------
// do_subset_dflt -- default method for `[`
// ---------------------------------------------------------------------------

/// Default method for `[`. Handles vector, matrix, and array subsetting.
unsafe fn language_subscript_keeps_all(x: SEXP, sub: SEXP) -> bool {
    unsafe {
        if sub.is_null() || sub == R_NilValue() || TYPEOF(sub) != SEXPTYPE::INTSXP {
            return false;
        }
        let n = length_int(x);
        if n <= 0 || XLENGTH(sub) != n as i64 {
            return false;
        }
        for i in 0..n {
            if *INTEGER(sub).add(i as usize) != i + 1 {
                return false;
            }
        }
        true
    }
}

pub unsafe fn do_subset_dflt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (op, rho);
        let _args_guard = protect(args);

        let mut drop: c_int = 1;
        ExtractDropArg(args, &mut drop);

        let x = CAR(args);

        /* Handle NULL case */
        if isNull(x) {
            return x;
        }

        let subs = CDR(args);
        {
            let mut cell = subs;
            while !isNull(cell) {
                if isPromise(CAR(cell)) {
                    SETCAR(cell, crate::sexp::envir::forcePromise(CAR(cell)));
                } else if isSymbol(CAR(cell)) && CAR(cell) != R_MissingArg() {
                    SETCAR(cell, crate::eval::eval::Rf_eval(CAR(cell), rho));
                }
                cell = CDR(cell);
            }
        }
        let nsubs = length_int(subs);
        let xtype = TYPEOF(x);
        let data_frame_subset = is_data_frame(x) && nsubs < 2 && xtype == SEXPTYPE::VECSXP;

        /* Coerce pair-based objects into generic vectors */
        let mut ax = x;
        let mut _ax_guard = None;
        if isVector(x) || isVectorList(x) {
            _ax_guard = Some(protect(ax));
        } else if isPairList(x) {
            let dim = getAttrib(x, sym_Dim());
            let ndim = length_int(dim);
            if ndim > 1 {
                ax = Rf_allocVector3(SEXPTYPE::VECSXP, xlength(x));
                _ax_guard = Some(protect(ax));
                setAttrib(ax, sym_DimNames(), getAttrib(x, sym_DimNames()));
                setAttrib(ax, sym_Names(), getAttrib(x, sym_DimNames()));
            } else {
                ax = Rf_allocVector3(SEXPTYPE::VECSXP, xlength(x));
                _ax_guard = Some(protect(ax));
                let names = getAttrib(x, sym_Names());
                let names = if isNull(names) || names == R_NilValue() {
                    pairlist_tag_names(x)
                } else {
                    names
                };
                setAttrib(ax, sym_Names(), names);
            }

            let mut px = x;
            let mut idx: R_xlen_t = 0;
            while !isNull(px) {
                SET_VECTOR_ELT(ax, idx, CAR(px));
                px = CDR(px);
                idx += 1;
            }
        } else {
            errorcallNotSubsettable(x, call);
        }

        /// Whether to take the `[.data.frame` emulation path: a data.frame with
        /// exactly two `[` subscripts (`df[i, j]`).
        unsafe fn data_frame_subset_2(x: SEXP, nsubs: c_int) -> bool {
            unsafe { nsubs == 2 && is_data_frame(x) && TYPEOF(x) == SEXPTYPE::VECSXP }
        }

        /// GNU `.row_names_info(x, 2L)`: compact `c(NA, ±n)` stores nrow in
        /// `|n|`. A 2-D AsIs column has length nrow*ncol, so `length(x[[1]])`
        /// is the wrong unit.
        unsafe fn data_frame_nrows(x: SEXP, first: SEXP) -> c_int {
            unsafe {
                let rownames = getAttrib(x, sym_RowNames());
                if TYPEOF(rownames) == SEXPTYPE::INTSXP
                    && length_int(rownames) == 2
                    && INTEGER_ELT(rownames, 0) == NA_INTEGER
                {
                    let n = INTEGER_ELT(rownames, 1);
                    return if n == c_int::MIN { 0 } else { n.abs() };
                }
                if !isNull(rownames) && length_int(rownames) > 0 {
                    return length_int(rownames);
                }
                if isMatrix(first) {
                    return nrows(first);
                }
                length_int(first)
            }
        }
        unsafe fn fix_na_row_names(rows: SEXP) -> SEXP {
            unsafe {
                if rows.is_null() || TYPEOF(rows) != SEXPTYPE::STRSXP {
                    return rows;
                }
                let n = XLENGTH(rows);
                let mut labels: Vec<Option<String>> = Vec::with_capacity(n as usize);
                let mut seen = std::collections::HashSet::new();
                let mut dup = false;
                let mut had_na = false;
                let mut has_na_label = false;
                for i in 0..n {
                    let s = STRING_ELT(rows, i);
                    if s.is_null() || s == crate::sexp::globals::R_NaString() {
                        had_na = true;
                        if !seen.insert(None) {
                            dup = true;
                        }
                        labels.push(None);
                    } else {
                        let text = std::ffi::CStr::from_ptr(CHAR(s))
                            .to_string_lossy()
                            .into_owned();
                        if text == "NA" {
                            has_na_label = true;
                        }
                        if !seen.insert(Some(text.clone())) {
                            dup = true;
                        }
                        labels.push(Some(text));
                    }
                }
                if !had_na && !dup {
                    return rows;
                }
                if !dup && has_na_label {
                    dup = true;
                }
                for label in &mut labels {
                    if label.is_none() {
                        *label = Some("NA".to_string());
                    }
                }
                let labels: Vec<String> = if dup {
                    let mut used = std::collections::HashSet::new();
                    for text in labels.iter().flatten() {
                        used.insert(text.clone());
                    }
                    let mut kept = std::collections::HashSet::new();
                    let mut next = std::collections::HashMap::new();
                    let mut out = Vec::with_capacity(labels.len());
                    for label in labels {
                        let base = label.unwrap_or_default();
                        if kept.insert(base.clone()) {
                            next.insert(base.clone(), 1u32);
                            out.push(base);
                            continue;
                        }
                        let mut k = next.get(&base).copied().unwrap_or(1);
                        loop {
                            let candidate = format!("{base}.{k}");
                            k += 1;
                            if used.insert(candidate.clone()) {
                                next.insert(base, k);
                                out.push(candidate);
                                break;
                            }
                        }
                    }
                    out
                } else {
                    labels.into_iter().map(|s| s.unwrap_or_default()).collect()
                };
                let ans = Rf_allocVector3(SEXPTYPE::STRSXP, n);
                let _g = protect(ans);
                for (i, label) in labels.iter().enumerate() {
                    let cstr = std::ffi::CString::new(label.as_str()).unwrap_or_default();
                    SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                }
                ans
            }
        }

        /// GNU `[.data.frame` row-subsets a column as
        /// `if (length(dim(xj)) != 2L) xj[i] else xj[i, , drop = FALSE]`.
        /// `[.factor` / `[.POSIXct` / `[.AsIs` restore class plus levels/tzone;
        /// `copyMostAttrib` is that transfer (names/dim/dimnames already set).
        unsafe fn subset_frame_column(col: SEXP, sr: SEXP, call: SEXP) -> SEXP {
            unsafe {
                let result = if isMatrix(col) {
                    let jcell = Rf_cons(R_MissingArg(), R_NilValue());
                    let _j_guard = protect(jcell);
                    let icell = Rf_cons(sr, jcell);
                    let _i_guard = protect(icell);
                    MatrixSubset(col, icell, call, 0)
                } else {
                    VectorSubset(col, sr, call)
                };
                let _result_guard = protect(result);
                crate::mainutils::array::copyMostAttrib(col, result);
                result
            }
        }

        /// Subset a data.frame's row.names for `df[i, ]`, handling the compact
        /// automatic form `c(NA_integer_, -n)` the way stock does: a contiguous
        /// `1..k` selection keeps the compact form, anything else expands to the
        /// selected row numbers as strings.
        unsafe fn subset_frame_rownames(rownames: SEXP, sr: SEXP, _nrows: c_int) -> SEXP {
            unsafe {
                if TYPEOF(rownames) == SEXPTYPE::INTSXP
                    && length_int(rownames) == 2
                    && INTEGER_ELT(rownames, 0) == NA_INTEGER
                {
                    let nsr = LENGTH(sr);
                    let mut contiguous = true;
                    for i in 0..nsr {
                        if INTEGER_ELT(sr, i) != i + 1 {
                            contiguous = false;
                            break;
                        }
                    }
                    if contiguous {
                        let ans = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
                        let _ans_guard = protect(ans);
                        *INTEGER(ans).add(0) = NA_INTEGER;
                        *INTEGER(ans).add(1) = -nsr;
                        return ans;
                    }
                    let ans = Rf_allocVector3(SEXPTYPE::STRSXP, nsr as R_xlen_t);
                    let _ans_guard = protect(ans);
                    for i in 0..nsr {
                        let idx = INTEGER_ELT(sr, i);
                        let label = if idx == NA_INTEGER {
                            "NA".to_string()
                        } else {
                            format!("{idx}")
                        };
                        let cstr = std::ffi::CString::new(label).unwrap_or_default();
                        SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                    }
                    return ans;
                }
                fix_na_row_names(VectorSubset(rownames, sr, std::ptr::null_mut()))
            }
        }

        /* data.frames with two subscripts: emulate [.data.frame (i, j).
        A single selected column with drop=TRUE returns the column
        itself; otherwise the result is a data.frame of the selected
        rows and columns. Both subscripts may be missing (`df[1,]`,
        `df[,1]`). */
        if data_frame_subset_2(x, nsubs) {
            let ncols = length_int(ax);
            let first = if ncols > 0 {
                VECTOR_ELT(ax, 0)
            } else {
                R_NilValue()
            };
            let nrows = data_frame_nrows(x, first);
            let dims = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            let _dims_guard = protect(dims);
            *INTEGER(dims).add(0) = nrows;
            *INTEGER(dims).add(1) = ncols;
            let rownames = getAttrib(x, sym_RowNames());
            let _rownames_guard = protect(rownames);
            let colnames = getAttrib(x, sym_Names());
            let _colnames_guard = protect(colnames);

            // int_arraySubscript resolves character indices through an
            // array's dimnames.  A data.frame stores the equivalent labels
            // as row.names and names, so expose that representation on a
            // shallow temporary rather than mutating the frame itself.
            let subscript_source = if TYPEOF(CAR(subs)) == SEXPTYPE::STRSXP
                || TYPEOF(CADR(subs)) == SEXPTYPE::STRSXP
            {
                let source = crate::mainutils::duplicate::shallow_duplicate(ax);
                let _source_guard = protect(source);
                let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
                let _dimnames_guard = protect(dimnames);
                SET_VECTOR_ELT(dimnames, 0, rownames);
                SET_VECTOR_ELT(dimnames, 1, colnames);
                setAttrib(source, sym_DimNames(), dimnames);
                source
            } else {
                ax
            };
            let _subscript_source_guard = protect(subscript_source);
            let row_arg = CAR(subs);
            let sr = if TYPEOF(row_arg) == SEXPTYPE::LGLSXP {
                // Vector `[` starts stretch at 1, so a length-1 FALSE on a
                // zero-row frame selects nothing. Array subscripting starts
                // stretch at 0 and rejects that as too long.
                let nrow = INTEGER_ELT(dims, 0) as R_xlen_t;
                let proxy = Rf_allocVector3(SEXPTYPE::REALSXP, nrow);
                let _proxy_guard = protect(proxy);
                let mut stretch: R_xlen_t = 1;
                makeSubscript(proxy, row_arg, &mut stretch, call)
            } else {
                int_arraySubscript(0, row_arg, dims, subscript_source, call)
            };
            let _sr_guard = protect(sr);
            let sc = int_arraySubscript(1, CADR(subs), dims, subscript_source, call);
            let _sc_guard = protect(sc);

            if LENGTH(sc) == 1 && drop != 0 {
                let col = VECTOR_ELT(ax, (*INTEGER(sc) as R_xlen_t) - 1);
                let _col_guard = protect(col);
                return subset_frame_column(col, sr, call);
            }

            let ncol = LENGTH(sc);
            let ans = Rf_allocVector3(SEXPTYPE::VECSXP, ncol as R_xlen_t);
            let _ans_guard = protect(ans);
            for j in 0..ncol {
                let col = VECTOR_ELT(ax, (*INTEGER(sc).add(j as usize) as R_xlen_t) - 1);
                let _col_guard = protect(col);
                SET_VECTOR_ELT(ans, j as R_xlen_t, subset_frame_column(col, sr, call));
            }
            if !isNull(colnames) {
                let new_names = ExtractSubset(colnames, sc, call);
                let _new_names_guard = protect(new_names);
                setAttrib(ans, sym_Names(), new_names);
            }
            if !isNull(rownames) {
                setAttrib(
                    ans,
                    sym_RowNames(),
                    subset_frame_rownames(rownames, sr, nrows),
                );
            }
            let terms_sym = Rf_install(c"terms".as_ptr());
            let terms = getAttrib(x, terms_sym);
            if !isNull(terms) {
                setAttrib(ans, terms_sym, terms);
            }
            let class_attr = getAttrib(x, sym_Class());
            let _class_guard = protect(class_attr);
            setAttrib(ans, sym_Class(), class_attr);
            return ans;
        }

        /* The actual subsetting code */
        let mut ans: SEXP = if nsubs < 2 {
            if data_frame_subset && nsubs == 1 {
                let i = CAR(subs);
                if TYPEOF(i) == SEXPTYPE::LGLSXP && XLENGTH(i) > xlength(ax) {
                    errorcall(call, "undefined columns selected");
                }
            }
            let dim = getAttrib(x, sym_Dim());
            let ndim = length_int(dim);
            let ans = VectorSubset(
                ax,
                if nsubs == 1 {
                    CAR(subs)
                } else {
                    R_MissingArg()
                },
                call,
            );
            let ans_guard = protect(ans);

            /* One-dimensional arrays should keep their dimension unless drop && len == 1 */
            if ndim == 1 {
                let len = length_int(ans);
                if drop == 0 || len > 1 {
                    let nm = getAttrib(ans, sym_Names());
                    let _nm_guard = protect(nm);
                    let attr = Rf_ScalarInteger(len);
                    let _attr_guard = protect(attr);
                    if !isNull(getAttrib(dim, sym_Names())) {
                        setAttrib(attr, sym_Names(), getAttrib(dim, sym_Names()));
                    }
                    setAttrib(ans, sym_Dim(), attr);
                    let attrib = getAttrib(x, sym_DimNames());
                    if !isNull(attrib) {
                        let nattrib = crate::mainutils::duplicate::duplicate(attrib);
                        let _nattrib_guard = protect(nattrib);
                        SET_VECTOR_ELT(nattrib, 0, nm);
                        setAttrib(ans, sym_DimNames(), nattrib);
                        setAttrib(ans, sym_Names(), R_NilValue());
                    }
                }
            }
            std::mem::drop(ans_guard);
            ans
        } else {
            if nsubs != length_int(getAttrib(x, sym_Dim())) {
                errorcall(call, "incorrect number of dimensions");
            }
            if nsubs == 2 {
                MatrixSubset(ax, subs, call, drop)
            } else {
                ArraySubset(ax, subs, call, drop)
            }
        };
        let _ans_guard = protect(ans);

        /* Convert back to LANGSXP if original was a language object */
        let _lang_ans_guard;
        if xtype == SEXPTYPE::LANGSXP {
            let sub = if nsubs == 1 { CAR(subs) } else { R_NilValue() };
            if language_subscript_keeps_all(x, sub) {
                return x;
            }
            ax = ans;
            ans = allocLang(length_int(ax));
            _lang_ans_guard = Some(protect(ans));
            if length_int(ax) > 0 {
                let mut px = ans;
                let mut idx: c_int = 0;
                while !isNull(px) {
                    SETCAR(px, VECTOR_ELT(ax, idx as R_xlen_t));
                    px = CDR(px);
                    idx += 1;
                }
                setAttrib(ans, sym_Dim(), getAttrib(ax, sym_Dim()));
                setAttrib(ans, sym_DimNames(), getAttrib(ax, sym_DimNames()));
                setAttrib(ans, sym_Names(), getAttrib(ax, sym_Names()));
                RAISE_NAMED(ans, NAMED(ax));
            }
        } else {
            _lang_ans_guard = None;
        }

        if data_frame_subset {
            setAttrib(ans, sym_Class(), getAttrib(x, sym_Class()));
            setAttrib(ans, sym_RowNames(), getAttrib(x, sym_RowNames()));
        }

        let preserve_date_class = crate::mainutils::essentials::sexp_has_class(x, "Date");
        let preserve_posixct_class = crate::mainutils::essentials::sexp_has_class(x, "POSIXct");
        let preserve_table_class = crate::mainutils::essentials::sexp_has_class(x, "table");

        /* Remove erroneous attributes */
        if !isNull(ATTRIB(ans)) {
            setAttrib(ans, sym_Tsp(), R_NilValue());
            if !data_frame_subset
                && !preserve_date_class
                && !preserve_posixct_class
                && !preserve_table_class
            {
                setAttrib(ans, sym_Class(), R_NilValue());
            }
        }

        if preserve_date_class || preserve_posixct_class || preserve_table_class {
            setAttrib(ans, sym_Class(), getAttrib(x, sym_Class()));
            if preserve_posixct_class {
                let tzone = getAttrib(x, sym_Tzone());
                if !isNull(tzone) {
                    setAttrib(ans, sym_Tzone(), tzone);
                }
            }
        }

        ans
    }
}
